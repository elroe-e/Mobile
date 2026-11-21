//! Reading and writing audio file tags and cover art.

use lofty::config::WriteOptions;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::*;
use lofty::tag::Tag;
use std::path::Path;

/// Metadata read from (or to be written into) a file.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TrackMeta {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub genre: Option<String>,
    pub year: Option<u32>,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    pub duration_secs: f64,
    pub has_cover: bool,
    pub embedded_lyrics: Option<String>,
    #[serde(default)]
    pub bit_depth: Option<u8>,
    #[serde(default)]
    pub sample_rate: Option<u32>,
}

fn clean(s: Option<std::borrow::Cow<'_, str>>) -> Option<String> {
    s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

pub fn read_meta(path: &Path) -> Result<TrackMeta, String> {
    let file = lofty::read_from_path(path).map_err(|e| e.to_string())?;
    let props = file.properties();
    let duration_secs = props.duration().as_secs_f64();
    let mut meta = TrackMeta {
        duration_secs,
        bit_depth: props.bit_depth(),
        sample_rate: props.sample_rate(),
        ..Default::default()
    };
    if let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) {
        meta.title = clean(tag.title());
        meta.artist = clean(tag.artist());
        meta.album = clean(tag.album());
        meta.genre = clean(tag.genre());
        meta.year = tag.year();
        meta.track_no = tag.track();
        meta.disc_no = tag.disk();
        meta.album_artist = tag
            .get_string(&ItemKey::AlbumArtist)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        meta.embedded_lyrics = tag
            .get_string(&ItemKey::Lyrics)
            .map(|s| s.to_string())
            .filter(|s| !s.trim().is_empty());
        meta.has_cover = !tag.pictures().is_empty();
    }
    Ok(meta)
}

/// Returns (bytes, mime type) of the embedded front cover, if any.
pub fn read_cover(path: &Path) -> Option<(Vec<u8>, String)> {
    let file = lofty::read_from_path(path).ok()?;
    let tag = file.primary_tag().or_else(|| file.first_tag())?;
    let pics = tag.pictures();
    let pic = pics
        .iter()
        .find(|p| p.pic_type() == PictureType::CoverFront)
        .or_else(|| pics.first())?;
    let mime = pic
        .mime_type()
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| "image/jpeg".into());
    Some((pic.data().to_vec(), mime))
}

/// Looks for cover.jpg / folder.jpg etc. next to the file.
pub fn find_folder_cover(path: &Path) -> Option<std::path::PathBuf> {
    let dir = path.parent()?;
    for name in [
        "cover", "folder", "front", "album", "Cover", "Folder", "Front", "AlbumArt",
    ] {
        for ext in ["jpg", "jpeg", "png", "webp"] {
            let p = dir.join(format!("{name}.{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Fill in missing tags (never overwrites values the file already has) and
/// optionally embed a cover image when the file has none.
pub fn write_missing(path: &Path, meta: &TrackMeta, cover: Option<&[u8]>) -> Result<(), String> {
    let mut file = lofty::read_from_path(path).map_err(|e| e.to_string())?;
    let tag_type = file.primary_tag_type();
    if file.primary_tag().is_none() {
        file.insert_tag(Tag::new(tag_type));
    }
    let tag = file.primary_tag_mut().ok_or("file does not support tags")?;

    if tag.title().is_none() {
        if let Some(v) = &meta.title {
            tag.set_title(v.clone());
        }
    }
    if tag.artist().is_none() {
        if let Some(v) = &meta.artist {
            tag.set_artist(v.clone());
        }
    }
    if tag.album().is_none() {
        if let Some(v) = &meta.album {
            tag.set_album(v.clone());
        }
    }
    if tag.genre().is_none() {
        if let Some(v) = &meta.genre {
            tag.set_genre(v.clone());
        }
    }
    if tag.year().is_none() {
        if let Some(v) = meta.year {
            tag.set_year(v);
        }
    }
    if tag.track().is_none() {
        if let Some(v) = meta.track_no {
            tag.set_track(v);
        }
    }
    if tag.get_string(&ItemKey::AlbumArtist).is_none() {
        if let Some(v) = &meta.album_artist {
            tag.insert_text(ItemKey::AlbumArtist, v.clone());
        }
    }
    if let Some(bytes) = cover {
        if tag.pictures().is_empty() {
            let mime = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
                MimeType::Png
            } else {
                MimeType::Jpeg
            };
            tag.push_picture(Picture::new_unchecked(
                PictureType::CoverFront,
                Some(mime),
                None,
                bytes.to_vec(),
            ));
        }
    }
    file.save_to_path(path, WriteOptions::default())
        .map_err(|e| e.to_string())
}
