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

/// The file's tags as lofty reads them; DSD files go through `dsd::tagged`.
fn open_tagged(path: &Path) -> Result<lofty::file::TaggedFile, String> {
    if crate::dsd::is_dsd(path) {
        let (_, id3) = crate::dsd::read(path).ok_or("not a DSD file")?;
        return id3
            .as_deref()
            .and_then(crate::dsd::tagged)
            .ok_or_else(|| "no tags".to_string());
    }
    lofty::read_from_path(path).map_err(|e| e.to_string())
}

pub fn read_meta(path: &Path) -> Result<TrackMeta, String> {
    if crate::dsd::is_dsd(path) {
        let (info, _) = crate::dsd::read(path).ok_or("not a DSD file")?;
        // Tags are optional; the header alone gives length and format.
        let mut meta = open_tagged(path).map(|f| meta_from(&f)).unwrap_or_default();
        meta.duration_secs = info.duration_secs();
        meta.sample_rate = Some(info.rate);
        meta.bit_depth = Some(1);
        return Ok(meta);
    }
    let file = open_tagged(path)?;
    let props = file.properties();
    let mut meta = meta_from(&file);
    meta.duration_secs = props.duration().as_secs_f64();
    meta.bit_depth = props.bit_depth();
    meta.sample_rate = props.sample_rate();
    Ok(meta)
}

fn meta_from(file: &lofty::file::TaggedFile) -> TrackMeta {
    let mut meta = TrackMeta::default();
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
    meta
}

/// A CUE sheet stored in the file's tags (FLAC, WavPack and APE keep one as CUESHEET).
pub fn embedded_cuesheet(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if !matches!(ext.as_str(), "flac" | "wv" | "ape") {
        return None;
    }
    let file = lofty::read_from_path(path).ok()?;
    let tag = file.primary_tag().or_else(|| file.first_tag())?;
    tag.get_string(&ItemKey::Unknown("CUESHEET".into()))
        .map(|s| s.to_string())
        .filter(|s| s.contains("TRACK"))
}

/// Returns (bytes, mime type) of the embedded front cover, if any.
pub fn read_cover(path: &Path) -> Option<(Vec<u8>, String)> {
    let file = open_tagged(path).ok()?;
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

#[cfg(test)]
mod generated_tests {
    use super::*;
    use std::path::PathBuf;

    /// Needs `scripts/make-fixtures.sh` (CI runs it).
    #[test]
    #[ignore]
    fn reads_tags_cover_and_lyrics_from_generated_fixture() {
        let dir = std::env::var_os("LOSS_FIXTURES")
            .map(PathBuf::from)
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures"));
        let m = read_meta(&dir.join("tagged-cover.flac")).expect("run scripts/make-fixtures.sh");
        assert_eq!(m.title.as_deref(), Some("Tagged Song"));
        assert_eq!(m.artist.as_deref(), Some("Loss Test Artist"));
        assert_eq!(m.album.as_deref(), Some("Loss Test Album"));
        assert_eq!(m.album_artist.as_deref(), Some("Loss Test Artist"));
        assert_eq!(
            (m.track_no, m.disc_no, m.year),
            (Some(3), Some(1), Some(2024))
        );
        assert!(m.has_cover);
        assert_eq!((m.bit_depth, m.sample_rate), (Some(16), Some(44100)));
        assert!((m.duration_secs - 2.0).abs() < 0.05);
        let lrc = std::fs::read_to_string(dir.join("tagged-cover.lrc")).unwrap();
        let lyrics = crate::lrc::parse(&lrc);
        assert!(lyrics.is_synced());
        assert_eq!(lyrics.current_line(1.5), Some(1));
    }
}
