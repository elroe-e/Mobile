//! After a download: unpack zips, fill in tags and cover art, fetch lyrics.

use crate::{lyrics, DownloadRequest};
use loss_core::{is_audio_file, tags};
use std::path::{Path, PathBuf};

pub struct PostOptions {
    pub auto_tag: bool,
    pub fetch_lyrics: bool,
    pub extract_zips: bool,
}

/// Extracts audio files and cover images from a zip into a folder named after
/// the archive. Returns the audio files.
pub fn extract_zip(zip_path: &Path) -> Result<Vec<PathBuf>, String> {
    let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let stem = zip_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("album");
    let dest = zip_path
        .parent()
        .unwrap_or(Path::new("."))
        .join(loss_core::sanitize_file_name(stem));
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    let mut audio = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        if entry.is_dir() {
            continue;
        }
        // enclosed_name() rejects "../" and absolute paths (zip-slip).
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let Some(file_name) = name.file_name() else {
            continue;
        };
        let out = dest.join(file_name);
        let lower = out.to_string_lossy().to_lowercase();
        let is_image = [".jpg", ".jpeg", ".png", ".webp"]
            .iter()
            .any(|e| lower.ends_with(e));
        if !is_audio_file(&out) && !is_image {
            continue;
        }
        let mut f = std::fs::File::create(&out).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut f).map_err(|e| e.to_string())?;
        if is_audio_file(&out) {
            audio.push(out);
        }
    }
    if audio.is_empty() {
        return Err("the zip file contains no audio files".into());
    }
    std::fs::remove_file(zip_path).ok();
    Ok(audio)
}

fn sidecar_lrc(path: &Path) -> PathBuf {
    path.with_extension("lrc")
}

/// Runs all post-processing steps; returns the final audio files.
pub async fn run(
    client: &reqwest::Client,
    path: PathBuf,
    req: &DownloadRequest,
    opts: &PostOptions,
) -> Result<Vec<PathBuf>, String> {
    let is_zip = path
        .extension()
        .map(|e| e.eq_ignore_ascii_case("zip"))
        .unwrap_or(false);
    let files = if is_zip && opts.extract_zips {
        let p = path.clone();
        tokio::task::spawn_blocking(move || extract_zip(&p))
            .await
            .map_err(|e| e.to_string())??
    } else {
        vec![path]
    };

    if !opts.auto_tag && !opts.fetch_lyrics {
        return Ok(files);
    }

    let cover = match (&req.cover_url, opts.auto_tag && !is_zip) {
        (Some(url), true) => crate::http::get_bytes(client, url).await.ok(),
        _ => None,
    };

    for file in &files {
        if !is_audio_file(file) {
            continue;
        }
        if opts.auto_tag && !is_zip {
            let (f, meta, cover) = (file.clone(), req.meta.clone(), cover.clone());
            let res = tokio::task::spawn_blocking(move || {
                tags::write_missing(&f, &meta, cover.as_deref())
            })
            .await
            .map_err(|e| e.to_string())?;
            if let Err(e) = res {
                log::warn!("could not tag {}: {e}", file.display());
            }
        }
        if opts.fetch_lyrics && !sidecar_lrc(file).exists() {
            let f = file.clone();
            let meta = tokio::task::spawn_blocking(move || tags::read_meta(&f))
                .await
                .map_err(|e| e.to_string())?
                .unwrap_or_default();
            if meta.embedded_lyrics.is_some() {
                continue;
            }
            let (Some(artist), Some(title)) = (
                meta.artist.or(req.meta.artist.clone()),
                meta.title.or(req.meta.title.clone()),
            ) else {
                continue;
            };
            let dur = (meta.duration_secs > 0.0).then_some(meta.duration_secs);
            match lyrics::fetch(client, &artist, &title, meta.album.as_deref(), dur).await {
                Ok(Some(found)) => {
                    if let Some(text) = found.best() {
                        tokio::fs::write(sidecar_lrc(file), text).await.ok();
                    }
                }
                Ok(None) => {}
                Err(e) => log::info!("lyrics lookup failed: {e}"),
            }
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn extracts_audio_and_blocks_zip_slip() {
        let dir = std::env::temp_dir().join(format!("loss-zip-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let zip_path = dir.join("Artist - Album.zip");
        {
            let f = std::fs::File::create(&zip_path).unwrap();
            let mut z = zip::ZipWriter::new(f);
            let opts = zip::write::SimpleFileOptions::default();
            z.start_file("01 Song.flac", opts).unwrap();
            z.write_all(b"fLaC").unwrap();
            z.start_file("cover.jpg", opts).unwrap();
            z.write_all(b"jpg").unwrap();
            z.start_file("../evil.mp3", opts).unwrap();
            z.write_all(b"x").unwrap();
            z.start_file("readme.txt", opts).unwrap();
            z.write_all(b"x").unwrap();
            z.finish().unwrap();
        }
        let files = extract_zip(&zip_path).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("Artist - Album/01 Song.flac"));
        assert!(dir.join("Artist - Album/cover.jpg").exists());
        assert!(!dir.join("evil.mp3").exists());
        assert!(!zip_path.exists());
        std::fs::remove_dir_all(dir).ok();
    }
}
