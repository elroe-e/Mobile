//! Import playlists (M3U/M3U8 or CSV exported from a streaming service) and
//! match them against the library, so you can see which songs you own.

use crate::library::Library;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct PlaylistEntry {
    pub artist: String,
    pub title: String,
    /// Path given directly in an M3U file, if any.
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchResult {
    pub entry: PlaylistEntry,
    pub track_id: Option<String>,
}

pub fn parse(text: &str) -> Vec<PlaylistEntry> {
    let looks_m3u = text.trim_start().starts_with("#EXTM3U")
        || text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .all(|l| l.starts_with('#') || !l.contains(','));
    if looks_m3u {
        parse_m3u(text)
    } else {
        parse_csv(text)
    }
}

fn parse_m3u(text: &str) -> Vec<PlaylistEntry> {
    let mut out = Vec::new();
    let mut pending: Option<(String, String)> = None;
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(info) = line.strip_prefix("#EXTINF:") {
            let name = info.split_once(',').map(|x| x.1).unwrap_or("");
            pending = Some(split_artist_title(name));
        } else if line.starts_with('#') {
            continue;
        } else {
            let (artist, title) = pending.take().unwrap_or_else(|| {
                let stem = Path::new(line)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(line);
                split_artist_title(stem)
            });
            out.push(PlaylistEntry {
                artist,
                title,
                path: Some(line.to_string()),
            });
        }
    }
    out
}

fn split_artist_title(s: &str) -> (String, String) {
    match s.split_once(" - ") {
        Some((a, t)) => (a.trim().to_string(), t.trim().to_string()),
        None => (String::new(), s.trim().to_string()),
    }
}

/// Splits one CSV line, honouring double quotes.
fn csv_fields(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' | ';' | '\t' if !quoted => fields.push(std::mem::take(&mut cur)),
            c => cur.push(c),
        }
    }
    fields.push(cur);
    fields.into_iter().map(|f| f.trim().to_string()).collect()
}

/// CSV with a header row. Recognises common column names used by
/// playlist exporters ("Track Name", "Artist Name(s)", "title", "artist"...).
fn parse_csv(text: &str) -> Vec<PlaylistEntry> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let Some(header) = lines.next() else {
        return vec![];
    };
    let cols: Vec<String> = csv_fields(header)
        .into_iter()
        .map(|c| c.to_lowercase())
        .collect();
    let find = |names: &[&str]| cols.iter().position(|c| names.iter().any(|n| c.contains(n)));
    let title_i = find(&["track name", "title", "song", "name"]);
    let artist_i = find(&["artist"]);
    let Some(title_i) = title_i else {
        return vec![];
    };
    lines
        .map(csv_fields)
        .filter_map(|f| {
            let title = f.get(title_i)?.clone();
            if title.is_empty() {
                return None;
            }
            let artist = artist_i
                .and_then(|i| f.get(i))
                .map(|a| a.split([';', '&']).next().unwrap_or("").trim().to_string())
                .unwrap_or_default();
            Some(PlaylistEntry {
                artist,
                title,
                path: None,
            })
        })
        .collect()
}

pub fn match_library(entries: Vec<PlaylistEntry>, lib: &Library) -> Vec<MatchResult> {
    entries
        .into_iter()
        .map(|entry| {
            let by_path = entry.path.as_ref().and_then(|p| {
                lib.tracks
                    .iter()
                    .find(|t| t.path.to_string_lossy() == p.as_str() || t.path.ends_with(p))
            });
            let track_id = by_path
                .or_else(|| lib.find(&entry.artist, &entry.title))
                .map(|t| t.id.clone());
            MatchResult { entry, track_id }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn m3u() {
        let e = parse("#EXTM3U\n#EXTINF:123,Band - Song\nmusic/song.mp3\nother/Artist - Tune.flac\n");
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].artist, "Band");
        assert_eq!(e[1].title, "Tune");
    }

    #[test]
    fn csv() {
        let e = parse("\"Track Name\",\"Artist Name(s)\",Album\n\"Hello, Again\",\"A;B\",X\n");
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].title, "Hello, Again");
        assert_eq!(e[0].artist, "A");
    }
}
