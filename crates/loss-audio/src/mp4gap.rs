//! Gapless playback for AAC and ALAC in MP4 (M2-08). Encoders put priming samples before the
//! music and padding after it; the file says how many in the edit list (`moov/trak/edts/elst`)
//! or, from iTunes, in the `iTunSMPB` tag. Symphonia 0.5 reads neither, so Loss does.
//! Only boxes are read (seeking past the media data), never the audio itself.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// The music inside the decoded stream, in frames from its first decoded frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trim {
    /// Priming frames to drop at the start.
    pub delay: u64,
    /// Frames of music after the priming; None to play to the end.
    pub length: Option<u64>,
}

impl Trim {
    pub const NONE: Trim = Trim {
        delay: 0,
        length: None,
    };

    /// The part of `count` frames starting at `start` that is music, as a range of offsets
    /// into those frames.
    pub fn keep(&self, start: u64, count: u64) -> std::ops::Range<u64> {
        let lo = self.delay.max(start);
        let hi = match self.length {
            Some(n) => (self.delay + n).min(start + count),
            None => start + count,
        };
        if hi <= lo {
            return 0..0;
        }
        lo - start..hi - start
    }
}

/// A box header: type, where its content starts and ends.
struct BoxHead {
    kind: [u8; 4],
    body: u64,
    end: u64,
}

fn read_head(f: &mut File, at: u64, limit: u64) -> Option<BoxHead> {
    if at + 8 > limit {
        return None;
    }
    f.seek(SeekFrom::Start(at)).ok()?;
    let mut h = [0u8; 8];
    f.read_exact(&mut h).ok()?;
    let size32 = u32::from_be_bytes([h[0], h[1], h[2], h[3]]) as u64;
    let kind = [h[4], h[5], h[6], h[7]];
    let (size, header) = match size32 {
        1 => {
            let mut b = [0u8; 8];
            f.read_exact(&mut b).ok()?;
            (u64::from_be_bytes(b), 16)
        }
        0 => (limit - at, 8),
        n => (n, 8),
    };
    if size < header || at + size > limit {
        return None;
    }
    Some(BoxHead {
        kind,
        body: at + header,
        end: at + size,
    })
}

/// The children of the box spanning `start..end`.
fn children(f: &mut File, start: u64, end: u64) -> Vec<BoxHead> {
    let mut out = Vec::new();
    let mut at = start;
    while let Some(b) = read_head(f, at, end) {
        at = b.end;
        out.push(b);
        if out.len() > 4096 {
            break;
        }
    }
    out
}

fn find<'a>(boxes: &'a [BoxHead], kind: &[u8; 4]) -> Option<&'a BoxHead> {
    boxes.iter().find(|b| &b.kind == kind)
}

fn body(f: &mut File, b: &BoxHead, max: u64) -> Option<Vec<u8>> {
    let len = (b.end - b.body).min(max);
    f.seek(SeekFrom::Start(b.body)).ok()?;
    let mut v = vec![0u8; len as usize];
    f.read_exact(&mut v).ok()?;
    Some(v)
}

fn be32(v: &[u8], at: usize) -> Option<u64> {
    v.get(at..at + 4)
        .map(|x| u32::from_be_bytes([x[0], x[1], x[2], x[3]]) as u64)
}

fn be64(v: &[u8], at: usize) -> Option<u64> {
    v.get(at..at + 8)
        .map(|x| u64::from_be_bytes([x[0], x[1], x[2], x[3], x[4], x[5], x[6], x[7]]))
}

/// mvhd / mdhd timescale (version 0 or 1).
fn timescale(v: &[u8]) -> Option<u64> {
    match v.first()? {
        1 => be32(v, 20),
        _ => be32(v, 12),
    }
}

/// `iTunSMPB`: " 00000000 DELAY PADDING LENGTH …" in hex.
pub fn parse_itunsmpb(text: &str) -> Option<Trim> {
    let words: Vec<u64> = text
        .split_whitespace()
        .map(|w| u64::from_str_radix(w, 16))
        .collect::<Result<_, _>>()
        .ok()?;
    let (delay, length) = (*words.get(1)?, *words.get(3)?);
    if delay == 0 && length == 0 {
        return None;
    }
    Some(Trim {
        delay,
        length: (length > 0).then_some(length),
    })
}

fn itunsmpb(f: &mut File, moov: &[BoxHead]) -> Option<Trim> {
    let udta = find(moov, b"udta")?;
    let udta_kids = children(f, udta.body, udta.end);
    let meta = find(&udta_kids, b"meta")?;
    // `meta` is a full box: 4 bytes of version and flags before its children.
    let meta_kids = children(f, meta.body + 4, meta.end);
    let ilst = find(&meta_kids, b"ilst")?;
    for item in children(f, ilst.body, ilst.end) {
        if &item.kind != b"----" {
            continue;
        }
        let parts = children(f, item.body, item.end);
        let name = find(&parts, b"name").and_then(|b| body(f, b, 64));
        if !name.is_some_and(|n| n.len() > 4 && &n[4..] == b"iTunSMPB") {
            continue;
        }
        let data = find(&parts, b"data").and_then(|b| body(f, b, 512))?;
        // data: 4 bytes type, 4 bytes locale, then the text.
        return parse_itunsmpb(std::str::from_utf8(data.get(8..)?).ok()?);
    }
    None
}

/// The edit list of the first sound track: (media start, playable duration in media frames).
fn edit_list(f: &mut File, moov: &[BoxHead]) -> Option<Trim> {
    let movie_scale = find(moov, b"mvhd")
        .and_then(|b| body(f, b, 32))
        .and_then(|v| timescale(&v))?;
    for trak in moov.iter().filter(|b| &b.kind == b"trak") {
        let kids = children(f, trak.body, trak.end);
        let mdia = find(&kids, b"mdia")?;
        let mdia_kids = children(f, mdia.body, mdia.end);
        let hdlr = find(&mdia_kids, b"hdlr").and_then(|b| body(f, b, 16))?;
        if hdlr.get(8..12) != Some(b"soun") {
            continue;
        }
        let media_scale = find(&mdia_kids, b"mdhd")
            .and_then(|b| body(f, b, 32))
            .and_then(|v| timescale(&v))?;
        let edts = find(&kids, b"edts")?;
        let edts_kids = children(f, edts.body, edts.end);
        let elst = body(f, find(&edts_kids, b"elst")?, 4096)?;
        let version = *elst.first()?;
        let count = be32(&elst, 4)?;
        let entry = if version == 1 { 20 } else { 12 };
        // Skip empty edits (media_time -1); the first real one gives the start and length.
        for i in 0..count.min(16) as usize {
            let at = 8 + i * entry;
            let (duration, media_time) = if version == 1 {
                (be64(&elst, at)?, be64(&elst, at + 8)? as i64)
            } else {
                (be32(&elst, at)?, be32(&elst, at + 4)? as u32 as i32 as i64)
            };
            if media_time < 0 {
                continue;
            }
            let length = (duration as u128 * media_scale as u128 + movie_scale as u128 / 2)
                / movie_scale.max(1) as u128;
            return Some(Trim {
                delay: media_time as u64,
                length: (length > 0).then_some(length as u64),
            });
        }
        return None;
    }
    None
}

/// The priming and length of an MP4 file's audio (iTunSMPB first, else the edit list), in
/// frames of its media timescale (the sample rate for audio tracks). None when the file says
/// nothing or isn't MP4.
pub fn read(path: &Path) -> Option<Trim> {
    let mut f = File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let top = children(&mut f, 0, len);
    // Not MP4 without a file-type box.
    find(&top, b"ftyp")?;
    let moov = find(&top, b"moov")?;
    let moov_kids = children(&mut f, moov.body, moov.end);
    itunsmpb(&mut f, &moov_kids)
        .or_else(|| edit_list(&mut f, &moov_kids))
        .filter(|t| *t != Trim::NONE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_itunes_gapless_info() {
        let t = parse_itunsmpb(
            " 00000000 00000840 000001CA 00000000000F3E36 00000000 00000000 00000000",
        )
        .unwrap();
        assert_eq!(t.delay, 0x840);
        assert_eq!(t.length, Some(0xF3E36));
        assert_eq!(
            parse_itunsmpb(" 00000000 00000000 00000000 0000000000000000"),
            None
        );
        assert_eq!(parse_itunsmpb("garbage"), None);
    }

    #[test]
    fn keeps_only_the_music() {
        let t = Trim {
            delay: 1024,
            length: Some(44100),
        };
        // The first packet is all priming; the second starts with it.
        assert_eq!(t.keep(0, 1024), 0..0);
        assert_eq!(t.keep(1000, 1024), 24..1024);
        // The packet holding the end keeps only what's before the padding.
        assert_eq!(t.keep(45000, 1024), 0..124);
        assert_eq!(t.keep(46000, 1024), 0..0);
        assert_eq!(Trim::NONE.keep(5, 10), 0..10);
    }
}
