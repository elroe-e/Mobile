//! Parser for synced lyrics in LRC format (`[mm:ss.xx] line`).

#[derive(Debug, Clone, PartialEq)]
pub struct LyricLine {
    pub time: f64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Lyrics {
    /// Sorted by time. Empty when the lyrics are plain (unsynced) text.
    pub synced: Vec<LyricLine>,
    pub plain: String,
}

impl Lyrics {
    pub fn is_synced(&self) -> bool {
        !self.synced.is_empty()
    }

    /// Index of the line that should be highlighted at `pos` seconds.
    pub fn current_line(&self, pos: f64) -> Option<usize> {
        let idx = self.synced.partition_point(|l| l.time <= pos);
        idx.checked_sub(1)
    }
}

fn parse_timestamp(s: &str) -> Option<f64> {
    let (m, rest) = s.split_once(':')?;
    let m: f64 = m.trim().parse().ok()?;
    let sec: f64 = rest.trim().replace(':', ".").parse().ok()?;
    Some(m * 60.0 + sec)
}

pub fn parse(text: &str) -> Lyrics {
    let mut synced = Vec::new();
    let mut plain = Vec::new();
    let mut offset = 0.0;
    for raw in text.lines() {
        let mut line = raw.trim();
        let mut times = Vec::new();
        while let Some(rest) = line.strip_prefix('[') {
            let Some(end) = rest.find(']') else { break };
            let tag = &rest[..end];
            if let Some(t) = parse_timestamp(tag) {
                times.push(t);
            } else if let Some(v) = tag.strip_prefix("offset:") {
                offset = v.trim().parse::<f64>().unwrap_or(0.0) / 1000.0;
            }
            line = rest[end + 1..].trim_start();
        }
        if times.is_empty() {
            if !raw.trim_start().starts_with('[') {
                plain.push(raw.trim().to_string());
            }
        } else {
            plain.push(line.to_string());
            for t in times {
                synced.push(LyricLine {
                    time: (t - offset).max(0.0),
                    text: line.to_string(),
                });
            }
        }
    }
    synced.sort_by(|a, b| a.time.total_cmp(&b.time));
    Lyrics {
        synced,
        plain: plain.join("\n").trim().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_synced() {
        let l = parse("[ar:X]\n[00:01.00]One\n[00:05.50][01:00.00]Two\n");
        assert_eq!(l.synced.len(), 3);
        assert_eq!(l.synced[1].text, "Two");
        assert_eq!(l.current_line(0.5), None);
        assert_eq!(l.current_line(6.0), Some(1));
        assert_eq!(l.current_line(100.0), Some(2));
    }

    #[test]
    fn parses_plain() {
        let l = parse("hello\nworld");
        assert!(!l.is_synced());
        assert_eq!(l.plain, "hello\nworld");
    }
}
