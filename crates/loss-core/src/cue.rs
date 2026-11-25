//! CUE sheets (M2-08): one audio file split into tracks, from a `.cue` file next to it or a
//! FLAC's embedded CUESHEET tag. Times are `mm:ss:ff` with 75 frames a second.

/// One track of a sheet: where it starts in its file (INDEX 01) and its own tags.
#[derive(Debug, Clone, PartialEq)]
pub struct CueTrack {
    pub number: u32,
    pub title: Option<String>,
    pub performer: Option<String>,
    /// Seconds from the start of the file.
    pub start: f64,
    /// Where the next track in the same file starts; None for the last.
    pub end: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CueFile {
    /// The audio file name as written in the sheet.
    pub name: String,
    pub tracks: Vec<CueTrack>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sheet {
    pub title: Option<String>,
    pub performer: Option<String>,
    pub date: Option<u32>,
    pub genre: Option<String>,
    pub files: Vec<CueFile>,
}

impl Sheet {
    /// The tracks of the file called `name` (case-insensitive, ignoring folders).
    pub fn tracks_for(&self, name: &str) -> Option<&[CueTrack]> {
        let want = base_name(name).to_lowercase();
        self.files
            .iter()
            .find(|f| base_name(&f.name).to_lowercase() == want)
            .map(|f| f.tracks.as_slice())
            .filter(|t| !t.is_empty())
    }
}

fn base_name(s: &str) -> &str {
    s.rsplit(['/', '\\']).next().unwrap_or(s)
}

/// The rest of a line as a value: quoted text, or the bare words.
fn value(rest: &str) -> String {
    let rest = rest.trim();
    match rest.strip_prefix('"') {
        Some(r) => r.rsplit_once('"').map(|(v, _)| v).unwrap_or(r).to_string(),
        None => rest.to_string(),
    }
}

/// `mm:ss:ff` → seconds.
fn time(s: &str) -> Option<f64> {
    let mut it = s.trim().split(':');
    let m: u64 = it.next()?.parse().ok()?;
    let sec: u64 = it.next()?.parse().ok()?;
    let f: u64 = it.next()?.parse().ok()?;
    Some(m as f64 * 60.0 + sec as f64 + f as f64 / 75.0)
}

pub fn parse(text: &str) -> Sheet {
    let mut sheet = Sheet::default();
    let text = text.trim_start_matches('\u{feff}');
    for line in text.lines() {
        let line = line.trim();
        let (cmd, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let in_track = sheet.files.last().is_some_and(|f| !f.tracks.is_empty());
        match cmd.to_ascii_uppercase().as_str() {
            "FILE" => {
                // `FILE "name.flac" WAVE`: the name is quoted or the first word.
                let name = if rest.trim_start().starts_with('"') {
                    value(rest)
                } else {
                    rest.split_whitespace().next().unwrap_or("").to_string()
                };
                sheet.files.push(CueFile {
                    name,
                    tracks: vec![],
                });
            }
            "TRACK" => {
                let mut parts = rest.split_whitespace();
                let number = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
                let audio = parts.next().is_none_or(|k| k.eq_ignore_ascii_case("AUDIO"));
                if let (Some(f), true) = (sheet.files.last_mut(), audio) {
                    f.tracks.push(CueTrack {
                        number,
                        title: None,
                        performer: None,
                        start: f64::NAN,
                        end: None,
                    });
                }
            }
            "TITLE" | "PERFORMER" => {
                let v = Some(value(rest)).filter(|v| !v.is_empty());
                let track = sheet.files.last_mut().and_then(|f| f.tracks.last_mut());
                match (cmd.to_ascii_uppercase().as_str(), in_track, track) {
                    ("TITLE", true, Some(t)) => t.title = v,
                    ("PERFORMER", true, Some(t)) => t.performer = v,
                    ("TITLE", _, _) => sheet.title = v,
                    _ => sheet.performer = v,
                }
            }
            "INDEX" => {
                let mut parts = rest.split_whitespace();
                let (n, t) = (parts.next(), parts.next().and_then(time));
                if let (Some("01" | "1"), Some(t)) = (n, t) {
                    if let Some(tr) = sheet.files.last_mut().and_then(|f| f.tracks.last_mut()) {
                        tr.start = t;
                    }
                }
            }
            "REM" => {
                let (key, v) = rest
                    .trim()
                    .split_once(char::is_whitespace)
                    .unwrap_or((rest, ""));
                match key.to_ascii_uppercase().as_str() {
                    "DATE" => sheet.date = value(v).get(..4).and_then(|y| y.parse().ok()),
                    "GENRE" => sheet.genre = Some(value(v)).filter(|g| !g.is_empty()),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    // Tracks without an INDEX 01 can't be placed; each ends where the next starts.
    for f in &mut sheet.files {
        f.tracks.retain(|t| t.start.is_finite());
        f.tracks.sort_by(|a, b| a.start.total_cmp(&b.start));
        let starts: Vec<f64> = f.tracks.iter().map(|t| t.start).collect();
        for (i, t) in f.tracks.iter_mut().enumerate() {
            t.end = starts.get(i + 1).copied();
        }
    }
    sheet
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALBUM: &str = r#"REM GENRE "Classical"
REM DATE 1994
PERFORMER "Loss Test Artist"
TITLE "Loss Test Album"
FILE "cue-album.flac" WAVE
  TRACK 01 AUDIO
    TITLE "First"
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE "Second"
    PERFORMER "Guest"
    INDEX 00 00:01:70
    INDEX 01 00:02:00
  TRACK 03 AUDIO
    TITLE "Third"
    INDEX 01 00:04:37
"#;

    #[test]
    fn reads_a_sheet() {
        let s = parse(ALBUM);
        assert_eq!(s.title.as_deref(), Some("Loss Test Album"));
        assert_eq!(s.performer.as_deref(), Some("Loss Test Artist"));
        assert_eq!(
            (s.date, s.genre.as_deref()),
            (Some(1994), Some("Classical"))
        );
        let t = s.tracks_for("CUE-ALBUM.FLAC").unwrap();
        assert_eq!(t.len(), 3);
        assert_eq!(t[1].title.as_deref(), Some("Second"));
        assert_eq!(t[1].performer.as_deref(), Some("Guest"));
        // INDEX 01, not the pregap's INDEX 00.
        assert_eq!((t[1].start, t[1].end), (2.0, Some(4.0 + 37.0 / 75.0)));
        assert_eq!(t[2].end, None);
        assert!(s.tracks_for("other.flac").is_none());
    }

    #[test]
    fn copes_with_odd_sheets() {
        let s = parse("\u{feff}FILE C:\\Music\\album.wav WAVE\r\nTRACK 1 AUDIO\r\nINDEX 1 01:00:00\r\nTRACK 2 AUDIO\r\nTRACK 3 MODE1/2352\r\n");
        let t = s.tracks_for("album.wav").unwrap();
        // Track 2 has no INDEX 01; track 3 isn't audio.
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].start, 60.0);
        assert!(parse("garbage").files.is_empty());
    }
}
