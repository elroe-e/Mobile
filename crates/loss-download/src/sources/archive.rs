//! Internet Archive (archive.org): millions of free audio recordings, many
//! under Creative Commons or in the public domain.

use super::{license_label, Collection, ItemAction, SearchItem, SourceKind};
use crate::http::{as_f64, as_string, as_u32, get_json};
use crate::DownloadRequest;
use loss_core::tags::TrackMeta;
use std::collections::HashMap;
use urlencoding::encode;

pub async fn search(
    client: &reqwest::Client,
    query: &str,
    cc_only: bool,
    page: u32,
) -> crate::Result<Vec<SearchItem>> {
    let mut q = format!(
        "({query}) AND mediatype:audio AND format:(FLAC OR \"VBR MP3\" OR MP3 OR \"Ogg Vorbis\")"
    );
    if cc_only {
        q.push_str(" AND (licenseurl:*creativecommons.org* OR licenseurl:*publicdomain*)");
    }
    let url = format!(
        "https://archive.org/advancedsearch.php?q={}&fl[]=identifier&fl[]=title&fl[]=creator&fl[]=year&fl[]=licenseurl&sort[]=downloads+desc&rows=40&page={}&output=json",
        encode(&q),
        page.max(1)
    );
    let v = get_json(client, &url).await?;
    let docs = v["response"]["docs"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    Ok(docs
        .iter()
        .filter_map(|d| {
            let id = as_string(&d["identifier"])?;
            let title = as_string(&d["title"]).unwrap_or_else(|| id.clone());
            let creator = as_string(&d["creator"]).unwrap_or_default();
            let year = as_string(&d["year"]).unwrap_or_default();
            let subtitle = [creator, year]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" · ");
            Some(SearchItem {
                cover_url: Some(format!("https://archive.org/services/img/{}", encode(&id))),
                license: as_string(&d["licenseurl"]).map(|l| license_label(&l)),
                format: None,
                duration: None,
                action: ItemAction::Open(Collection {
                    source: SourceKind::InternetArchive,
                    id,
                    title: title.clone(),
                }),
                title,
                subtitle,
            })
        })
        .collect())
}

/// Quality ranking of archive.org file formats (higher is better).
fn format_rank(format: &str) -> Option<u8> {
    let f = format.to_lowercase();
    Some(if f.contains("flac") {
        6
    } else if f == "vbr mp3" || f.contains("320") {
        5
    } else if f.contains("mp3") {
        4
    } else if f.contains("ogg") {
        3
    } else if f.contains("opus") {
        2
    } else if f.contains("aiff") || f.contains("wave") {
        1
    } else {
        return None;
    })
}

fn ext_of(name: &str) -> &str {
    name.rsplit_once('.').map(|x| x.1).unwrap_or("")
}

/// Lists the audio files of one archive.org item, best format per track.
pub async fn open(client: &reqwest::Client, id: &str) -> crate::Result<Vec<SearchItem>> {
    let v = get_json(
        client,
        &format!("https://archive.org/metadata/{}", encode(id)),
    )
    .await?;
    let md = &v["metadata"];
    let item_title = as_string(&md["title"]).unwrap_or_else(|| id.to_string());
    let item_creator = as_string(&md["creator"]);
    let item_year = as_string(&md["year"])
        .or_else(|| as_string(&md["date"]))
        .and_then(|y| y.get(..4).and_then(|s| s.parse().ok()));
    let license = as_string(&md["licenseurl"]).map(|l| license_label(&l));
    let cover = format!("https://archive.org/services/img/{}", encode(id));
    let files = v["files"].as_array().cloned().unwrap_or_default();

    // Group originals and their derivatives by base name; keep the best.
    let mut best: HashMap<String, (u8, serde_json::Value)> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for f in files {
        let Some(name) = as_string(&f["name"]) else {
            continue;
        };
        let Some(rank) = as_string(&f["format"]).and_then(|fm| format_rank(&fm)) else {
            continue;
        };
        let base = name
            .rsplit_once('.')
            .map(|x| x.0)
            .unwrap_or(&name)
            .trim_end_matches("_vbr")
            .to_string();
        if !best.contains_key(&base) {
            order.push(base.clone());
        }
        let entry = best.entry(base).or_insert((0, serde_json::Value::Null));
        if rank > entry.0 {
            *entry = (rank, f);
        }
    }

    let mut items: Vec<(u32, SearchItem)> = order
        .into_iter()
        .enumerate()
        .filter_map(|(pos, base)| {
            let (_, f) = best.remove(&base)?;
            let name = as_string(&f["name"])?;
            let format = as_string(&f["format"]);
            let track_no = as_u32(&f["track"]);
            let title = as_string(&f["title"])
                .unwrap_or_else(|| base.rsplit('/').next().unwrap_or(&base).to_string());
            let meta = TrackMeta {
                title: Some(title.clone()),
                artist: as_string(&f["creator"])
                    .or_else(|| as_string(&f["artist"]))
                    .or(item_creator.clone()),
                album: as_string(&f["album"]).or(Some(item_title.clone())),
                album_artist: item_creator.clone(),
                year: item_year,
                track_no,
                ..Default::default()
            };
            let rel_path = DownloadRequest::build_rel_path(&meta, &title, ext_of(&name));
            let url = format!(
                "https://archive.org/download/{}/{}",
                encode(id),
                name.split('/')
                    .map(|p| encode(p).into_owned())
                    .collect::<Vec<_>>()
                    .join("/")
            );
            let duration = as_f64(&f["length"]);
            let subtitle = meta.artist.clone().unwrap_or_default();
            Some((
                track_no.unwrap_or(10_000 + pos as u32),
                SearchItem {
                    title,
                    subtitle,
                    cover_url: Some(cover.clone()),
                    license: license.clone(),
                    format,
                    duration,
                    action: ItemAction::Download(Box::new(DownloadRequest {
                        url,
                        rel_path,
                        meta,
                        cover_url: Some(cover.clone()),
                        source: SourceKind::InternetArchive,
                        headers: vec![],
                    })),
                },
            ))
        })
        .collect();
    items.sort_by_key(|(n, _)| *n);
    Ok(items.into_iter().map(|(_, i)| i).collect())
}
