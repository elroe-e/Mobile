//! The music library: tracks found in the user's folders plus downloads.

use crate::tags::{self, TrackMeta};
use crate::{is_audio_file, normalize, now_secs, stable_id};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TrackOrigin {
    #[default]
    Local,
    Downloaded,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub id: String,
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub genre: Option<String>,
    pub year: Option<u32>,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    pub duration_secs: f64,
    pub has_cover: bool,
    pub origin: TrackOrigin,
    pub added_at: u64,
    /// File modification time when it was last scanned (for incremental rescans).
    pub mtime: u64,
    #[serde(default)]
    pub play_count: u32,
    /// Container/codec from the extension, e.g. "FLAC". Empty in libraries
    /// saved by older versions (those tracks are re-read on the next scan).
    #[serde(default)]
    pub format: String,
    #[serde(default)]
    pub bit_depth: Option<u8>,
    #[serde(default)]
    pub sample_rate: Option<u32>,
    #[serde(default)]
    pub last_played: u64,
}

impl Track {
    pub fn from_file(path: &Path, origin: TrackOrigin) -> Result<Track, String> {
        let meta = tags::read_meta(path).unwrap_or_else(|e| {
            log::warn!("no tags for {}: {e}", path.display());
            TrackMeta::default()
        });
        Ok(Self::from_meta(path, meta, origin))
    }

    pub fn from_meta(path: &Path, meta: TrackMeta, origin: TrackOrigin) -> Track {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown")
            .to_string();
        // Fallback "Artist - Title" from the file name.
        let (guess_artist, guess_title) = match stem.split_once(" - ") {
            Some((a, t)) => (Some(a.trim().to_string()), t.trim().to_string()),
            None => (None, stem.clone()),
        };
        let artist = meta
            .artist
            .or(guess_artist)
            .unwrap_or_else(|| "Unknown Artist".into());
        let album = meta.album.unwrap_or_else(|| {
            path.parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                .unwrap_or("Unknown Album")
                .to_string()
        });
        let has_cover = meta.has_cover || tags::find_folder_cover(path).is_some();
        let format = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| match e.to_ascii_lowercase().as_str() {
                "m4a" | "mp4" | "aac" => "AAC".to_string(),
                "oga" => "OGG".to_string(),
                "aif" => "AIFF".to_string(),
                other => other.to_uppercase(),
            })
            .unwrap_or_default();
        Track {
            id: stable_id(&path.to_string_lossy()),
            path: path.to_path_buf(),
            title: meta.title.unwrap_or(guess_title),
            album_artist: meta.album_artist.unwrap_or_else(|| artist.clone()),
            artist,
            album,
            genre: meta.genre,
            year: meta.year,
            track_no: meta.track_no,
            disc_no: meta.disc_no,
            duration_secs: meta.duration_secs,
            has_cover,
            origin,
            added_at: now_secs(),
            mtime: file_mtime(path),
            play_count: 0,
            format,
            bit_depth: meta.bit_depth,
            sample_rate: meta.sample_rate,
            last_played: 0,
        }
    }

    pub fn is_lossless(&self) -> bool {
        matches!(
            self.format.as_str(),
            "FLAC" | "WAV" | "AIFF" | "ALAC" | "APE" | "WV"
        ) || (self.format == "AAC" && self.bit_depth.is_some())
    }

    /// Short quality badge: "Hi-Res", "24-bit", "FLAC", "MP3"...
    pub fn quality_badge(&self) -> String {
        let hires = self.sample_rate.map(|r| r > 48_000).unwrap_or(false);
        match (self.is_lossless(), self.bit_depth) {
            (true, Some(b)) if b >= 24 && hires => "Hi-Res".into(),
            (true, Some(b)) if b >= 24 => "24-bit".into(),
            (true, Some(b)) => format!("{b}-bit"),
            _ if self.format.is_empty() => String::new(),
            _ => self.format.clone(),
        }
    }

    pub fn album_key(&self) -> String {
        stable_id(&format!(
            "{}\u{1}{}",
            normalize(&self.album_artist),
            normalize(&self.album)
        ))
    }
}

fn file_mtime(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Album {
    pub key: String,
    pub title: String,
    pub artist: String,
    pub year: Option<u32>,
    /// Track ids, sorted by disc and track number.
    pub tracks: Vec<String>,
    /// A track whose cover can represent the album.
    pub cover_track: Option<String>,
    pub duration_secs: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Artist {
    pub name: String,
    pub album_keys: Vec<String>,
    pub track_count: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScanReport {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub total: usize,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Library {
    pub tracks: Vec<Track>,
    #[serde(skip)]
    index: HashMap<String, usize>,
}

impl Library {
    pub fn load(path: &Path) -> Library {
        let mut lib: Library = std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        lib.reindex();
        lib
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_vec(self)?)?;
        std::fs::rename(tmp, path)
    }

    /// A library from stored tracks (sorted and indexed).
    pub fn from_tracks(tracks: Vec<Track>) -> Library {
        let mut lib = Library {
            tracks,
            index: HashMap::new(),
        };
        lib.reindex();
        lib
    }

    fn reindex(&mut self) {
        // Keys are computed once per track (normalising is the slow part), which keeps a
        // 50,000-track library quick to load.
        self.tracks.sort_by_cached_key(|t| {
            (
                normalize(&t.artist),
                normalize(&t.album),
                t.disc_no.unwrap_or(1),
                t.track_no.unwrap_or(0),
                t.title.clone(),
            )
        });
        self.index = self
            .tracks
            .iter()
            .enumerate()
            .map(|(i, t)| (t.id.clone(), i))
            .collect();
    }

    pub fn get(&self, id: &str) -> Option<&Track> {
        self.index.get(id).map(|&i| &self.tracks[i])
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    /// Add or refresh a single file (used after a download finishes).
    pub fn add_file(&mut self, path: &Path, origin: TrackOrigin) -> Result<String, String> {
        let track = Track::from_file(path, origin)?;
        let id = track.id.clone();
        if let Some(&i) = self.index.get(&id) {
            let old = &self.tracks[i];
            let mut t = track;
            t.added_at = old.added_at;
            t.play_count = old.play_count;
            t.last_played = old.last_played;
            self.tracks[i] = t;
        } else {
            self.tracks.push(track);
        }
        self.reindex();
        Ok(id)
    }

    pub fn remove(&mut self, id: &str) {
        self.tracks.retain(|t| t.id != id);
        self.reindex();
    }

    pub fn record_play(&mut self, id: &str) {
        if let Some(&i) = self.index.get(id) {
            self.tracks[i].play_count += 1;
            self.tracks[i].last_played = now_secs();
        }
    }

    /// Recently played tracks, newest first.
    pub fn recently_played(&self, n: usize) -> Vec<&Track> {
        let mut v: Vec<&Track> = self.tracks.iter().filter(|t| t.last_played > 0).collect();
        v.sort_by_key(|t| std::cmp::Reverse(t.last_played));
        v.truncate(n);
        v
    }

    /// Incremental scan: new files are added, changed files re-read, and
    /// files that no longer exist are dropped. `extra_dirs` (e.g. the
    /// download folder) are scanned too, with their tracks marked Downloaded.
    pub fn scan(
        &mut self,
        folders: &[PathBuf],
        download_dir: Option<&Path>,
        mut progress: impl FnMut(usize),
    ) -> ScanReport {
        let mut report = ScanReport::default();
        let existing: HashMap<PathBuf, usize> = self
            .tracks
            .iter()
            .enumerate()
            .map(|(i, t)| (t.path.clone(), i))
            .collect();
        let mut seen: HashSet<PathBuf> = HashSet::new();
        let mut new_tracks: Vec<Track> = Vec::new();

        let mut roots: Vec<(PathBuf, TrackOrigin)> = folders
            .iter()
            .map(|f| (f.clone(), TrackOrigin::Local))
            .collect();
        if let Some(d) = download_dir {
            roots.push((d.to_path_buf(), TrackOrigin::Downloaded));
        }

        for (root, origin) in roots {
            for entry in walkdir::WalkDir::new(&root)
                .follow_links(true)
                .into_iter()
                .filter_entry(|e| {
                    e.file_name()
                        .to_str()
                        .map(|s| !s.starts_with('.'))
                        .unwrap_or(true)
                })
            {
                let entry = match entry {
                    Ok(e) => e,
                    Err(e) => {
                        report.errors.push(e.to_string());
                        continue;
                    }
                };
                let path = entry.path();
                if !entry.file_type().is_file() || !is_audio_file(path) {
                    continue;
                }
                if !seen.insert(path.to_path_buf()) {
                    continue;
                }
                let mtime = file_mtime(path);
                match existing.get(path) {
                    Some(&i)
                        if self.tracks[i].mtime == mtime && !self.tracks[i].format.is_empty() =>
                    {
                        new_tracks.push(self.tracks[i].clone());
                    }
                    Some(&i) => {
                        let mut t = Track::from_meta(
                            path,
                            tags::read_meta(path).unwrap_or_default(),
                            origin,
                        );
                        t.added_at = self.tracks[i].added_at;
                        t.play_count = self.tracks[i].play_count;
                        t.last_played = self.tracks[i].last_played;
                        new_tracks.push(t);
                        report.updated += 1;
                    }
                    None => {
                        new_tracks.push(Track::from_meta(
                            path,
                            tags::read_meta(path).unwrap_or_default(),
                            origin,
                        ));
                        report.added += 1;
                    }
                }
                progress(new_tracks.len());
            }
        }
        report.removed = self.tracks.len() + report.added - new_tracks.len();
        report.total = new_tracks.len();
        self.tracks = new_tracks;
        self.reindex();
        report
    }

    /// Simple substring search over title, artist and album.
    pub fn search(&self, query: &str) -> Vec<&Track> {
        let q = normalize(query);
        if q.is_empty() {
            return self.tracks.iter().collect();
        }
        let words: Vec<&str> = q.split(' ').collect();
        self.tracks
            .iter()
            .filter(|t| {
                let hay = normalize(&format!(
                    "{} {} {} {}",
                    t.title, t.artist, t.album, t.album_artist
                ));
                words.iter().all(|w| hay.contains(w))
            })
            .collect()
    }

    pub fn albums(&self) -> Vec<Album> {
        let mut map: BTreeMap<(String, String), Album> = BTreeMap::new();
        for t in &self.tracks {
            let key = (normalize(&t.album_artist), normalize(&t.album));
            let a = map.entry(key).or_insert_with(|| Album {
                key: t.album_key(),
                title: t.album.clone(),
                artist: t.album_artist.clone(),
                year: t.year,
                tracks: vec![],
                cover_track: None,
                duration_secs: 0.0,
            });
            a.tracks.push(t.id.clone());
            a.duration_secs += t.duration_secs;
            if a.year.is_none() {
                a.year = t.year;
            }
            if a.cover_track.is_none() && t.has_cover {
                a.cover_track = Some(t.id.clone());
            }
        }
        map.into_values().collect()
    }

    pub fn album(&self, key: &str) -> Option<Album> {
        self.albums().into_iter().find(|a| a.key == key)
    }

    pub fn artists(&self) -> Vec<Artist> {
        let mut map: BTreeMap<String, Artist> = BTreeMap::new();
        for a in self.albums() {
            let e = map.entry(normalize(&a.artist)).or_insert_with(|| Artist {
                name: a.artist.clone(),
                album_keys: vec![],
                track_count: 0,
            });
            e.track_count += a.tracks.len();
            e.album_keys.push(a.key.clone());
        }
        map.into_values().collect()
    }

    pub fn recently_added(&self, n: usize) -> Vec<&Track> {
        let mut v: Vec<&Track> = self.tracks.iter().collect();
        v.sort_by_key(|t| std::cmp::Reverse(t.added_at));
        v.truncate(n);
        v
    }

    pub fn most_played(&self, n: usize) -> Vec<&Track> {
        let mut v: Vec<&Track> = self.tracks.iter().filter(|t| t.play_count > 0).collect();
        v.sort_by_key(|t| std::cmp::Reverse(t.play_count));
        v.truncate(n);
        v
    }

    /// Find a track by fuzzy artist + title (used for playlist import).
    pub fn find(&self, artist: &str, title: &str) -> Option<&Track> {
        let (na, nt) = (normalize(artist), normalize(title));
        self.tracks
            .iter()
            .find(|t| normalize(&t.title) == nt && (na.is_empty() || normalize(&t.artist) == na))
            .or_else(|| {
                self.tracks.iter().find(|t| {
                    let tt = normalize(&t.title);
                    (tt.contains(&nt) || nt.contains(&tt))
                        && !tt.is_empty()
                        && (na.is_empty() || normalize(&t.artist).contains(&na))
                })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make(path: &str, artist: &str, album: &str, title: &str, n: u32) -> Track {
        let meta = TrackMeta {
            title: Some(title.into()),
            artist: Some(artist.into()),
            album: Some(album.into()),
            track_no: Some(n),
            ..Default::default()
        };
        Track::from_meta(Path::new(path), meta, TrackOrigin::Local)
    }

    #[test]
    fn groups_albums_and_searches() {
        let mut lib = Library {
            tracks: vec![
                make("/m/a/2.mp3", "Band", "First", "Second Song", 2),
                make("/m/a/1.mp3", "Band", "First", "Opening", 1),
                make("/m/b/1.mp3", "Other", "Solo", "Alone", 1),
            ],
            ..Default::default()
        };
        lib.reindex();
        let albums = lib.albums();
        assert_eq!(albums.len(), 2);
        let first = albums.iter().find(|a| a.title == "First").unwrap();
        assert_eq!(lib.get(&first.tracks[0]).unwrap().title, "Opening");
        assert_eq!(lib.search("band second").len(), 1);
        assert_eq!(lib.artists().len(), 2);
        assert!(lib.find("band", "opening").is_some());
    }

    #[test]
    fn quality_badges() {
        let mut t = make("/m/a.flac", "A", "B", "C", 1);
        t.bit_depth = Some(24);
        t.sample_rate = Some(96_000);
        assert_eq!(t.format, "FLAC");
        assert_eq!(t.quality_badge(), "Hi-Res");
        t.sample_rate = Some(48_000);
        assert_eq!(t.quality_badge(), "24-bit");
        t.bit_depth = Some(16);
        assert_eq!(t.quality_badge(), "16-bit");
        let m = make("/m/a.mp3", "A", "B", "C", 1);
        assert_eq!(m.quality_badge(), "MP3");
    }

    #[test]
    fn filename_fallback() {
        let t = Track::from_meta(
            Path::new("/x/Some Artist - Cool Tune.flac"),
            TrackMeta::default(),
            TrackOrigin::Local,
        );
        assert_eq!(t.artist, "Some Artist");
        assert_eq!(t.title, "Cool Tune");
        assert_eq!(t.album, "x");
    }
}
