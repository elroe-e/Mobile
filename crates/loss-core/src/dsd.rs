//! DSD files (DSF, DFF) for the library (M2-08): rate, channels and length from the header,
//! and tags from the file's ID3v2 block. lofty doesn't read DSD containers, but it reads ID3v2
//! inside WAV, so the block is wrapped in a minimal in-memory WAV for it.

use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::Path;

/// What the header says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DsdInfo {
    /// DSD sample rate in Hz (2,822,400 for DSD64).
    pub rate: u32,
    pub channels: u32,
    /// DSD samples (bits) per channel.
    pub samples: u64,
}

impl DsdInfo {
    pub fn duration_secs(&self) -> f64 {
        self.samples as f64 / self.rate.max(1) as f64
    }
}

pub fn is_dsd(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("dsf") || e.eq_ignore_ascii_case("dff"))
}

fn take<R: Read>(r: &mut R, n: usize) -> Option<Vec<u8>> {
    let mut v = vec![0u8; n];
    r.read_exact(&mut v).ok()?;
    Some(v)
}

fn le(b: &[u8]) -> u64 {
    b.iter().rev().fold(0, |a, &x| (a << 8) | x as u64)
}

fn be(b: &[u8]) -> u64 {
    b.iter().fold(0, |a, &x| (a << 8) | x as u64)
}

/// Header facts and the ID3v2 block, if any.
pub fn read(path: &Path) -> Option<(DsdInfo, Option<Vec<u8>>)> {
    let mut f = File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let magic = take(&mut f, 4)?;
    match &magic[..] {
        b"DSD " => {
            let head = take(&mut f, 24)?;
            let meta_at = le(&head[16..24]);
            let fmt = take(&mut f, 52)?;
            if &fmt[0..4] != b"fmt " {
                return None;
            }
            let info = DsdInfo {
                channels: le(&fmt[24..28]) as u32,
                rate: le(&fmt[28..32]) as u32,
                samples: le(&fmt[36..44]),
            };
            let id3 = (meta_at > 0 && meta_at + 10 <= len)
                .then(|| {
                    f.seek(SeekFrom::Start(meta_at)).ok()?;
                    take(&mut f, (len - meta_at).min(64 << 20) as usize)
                })
                .flatten();
            Some((info, id3))
        }
        b"FRM8" => {
            let head = take(&mut f, 12)?;
            if &head[8..12] != b"DSD " {
                return None;
            }
            let end = (12 + be(&head[0..8])).min(len);
            let (mut rate, mut channels, mut data, mut id3) = (0u32, 0u32, 0u64, None);
            let mut at = 16u64;
            while at + 12 <= end {
                f.seek(SeekFrom::Start(at)).ok()?;
                let ck = take(&mut f, 12)?;
                let size = be(&ck[4..12]);
                let body = at + 12;
                match &ck[0..4] {
                    b"PROP" => {
                        let mut sub = body + 4;
                        while sub + 12 <= body + size {
                            f.seek(SeekFrom::Start(sub)).ok()?;
                            let sc = take(&mut f, 12)?;
                            let ssize = be(&sc[4..12]);
                            match &sc[0..4] {
                                b"FS  " => rate = be(&take(&mut f, 4)?) as u32,
                                b"CHNL" => channels = be(&take(&mut f, 2)?) as u32,
                                _ => {}
                            }
                            sub += 12 + ssize + (ssize & 1);
                        }
                    }
                    b"DSD " | b"DST " => data = size,
                    b"ID3 " => id3 = take(&mut f, size.min(64 << 20) as usize),
                    _ => {}
                }
                at = body + size + (size & 1);
            }
            (rate > 0 && channels > 0).then(|| {
                (
                    DsdInfo {
                        rate,
                        channels,
                        samples: data * 8 / channels as u64,
                    },
                    id3,
                )
            })
        }
        _ => None,
    }
}

/// The ID3v2 block as a tagged file lofty can read (a WAV with no audio and an `id3 ` chunk).
pub fn tagged(id3: &[u8]) -> Option<lofty::file::TaggedFile> {
    use lofty::file::FileType;
    let mut fmt = Vec::new();
    fmt.extend(1u16.to_le_bytes()); // PCM
    fmt.extend(2u16.to_le_bytes());
    fmt.extend(44_100u32.to_le_bytes());
    fmt.extend((44_100u32 * 4).to_le_bytes());
    fmt.extend(4u16.to_le_bytes());
    fmt.extend(16u16.to_le_bytes());
    let mut body = Vec::new();
    body.extend(b"WAVE");
    body.extend(b"fmt ");
    body.extend((fmt.len() as u32).to_le_bytes());
    body.extend(fmt);
    // One silent frame: lofty wants a non-empty data chunk.
    body.extend(b"data");
    body.extend(4u32.to_le_bytes());
    body.extend([0u8; 4]);
    body.extend(b"id3 ");
    body.extend((id3.len() as u32).to_le_bytes());
    body.extend(id3);
    if id3.len() % 2 == 1 {
        body.push(0);
    }
    let mut wav = Vec::new();
    wav.extend(b"RIFF");
    wav.extend((body.len() as u32).to_le_bytes());
    wav.extend(body);
    lofty::probe::Probe::new(Cursor::new(wav))
        .set_file_type(FileType::Wav)
        .read()
        .map_err(|e| log::debug!("DSD tags unreadable: {e}"))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lofty::prelude::*;

    /// An ID3v2.3 tag with a title and an artist.
    fn id3() -> Vec<u8> {
        let frame = |id: &[u8; 4], text: &str| {
            let mut f = Vec::new();
            f.extend(id);
            f.extend(((text.len() + 1) as u32).to_be_bytes());
            f.extend([0, 0, 0]); // flags, ISO-8859-1
            f.extend(text.as_bytes());
            f
        };
        let mut frames = frame(b"TIT2", "Spem in alium");
        frames.extend(frame(b"TPE1", "Tallis Scholars"));
        let size = frames.len() as u32;
        // Synchsafe size.
        let ss = [
            (size >> 21) & 0x7f,
            (size >> 14) & 0x7f,
            (size >> 7) & 0x7f,
            size & 0x7f,
        ];
        let mut t = b"ID3\x03\x00\x00".to_vec();
        t.extend(ss.iter().map(|&b| b as u8));
        t.extend(frames);
        t
    }

    #[test]
    fn reads_dsf_header_and_tags() {
        let samples: u64 = 2_822_400 * 3; // 3 s of DSD64
        let data = vec![0x69u8; 4096 * 2];
        let tag = id3();
        let meta_at = 28 + 52 + 12 + data.len() as u64;
        let mut f = Vec::new();
        f.extend(b"DSD ");
        f.extend(28u64.to_le_bytes());
        f.extend((meta_at + tag.len() as u64).to_le_bytes());
        f.extend(meta_at.to_le_bytes());
        f.extend(b"fmt ");
        f.extend(52u64.to_le_bytes());
        for v in [1u32, 0, 2, 2, 2_822_400, 1] {
            f.extend(v.to_le_bytes());
        }
        f.extend(samples.to_le_bytes());
        f.extend(4096u32.to_le_bytes());
        f.extend(0u32.to_le_bytes());
        f.extend(b"data");
        f.extend((12 + data.len() as u64).to_le_bytes());
        f.extend(&data);
        f.extend(&tag);
        let path = std::env::temp_dir().join(format!("loss-core-dsd-{}.dsf", std::process::id()));
        std::fs::write(&path, f).unwrap();
        let (info, id3) = read(&path).unwrap();
        assert_eq!((info.rate, info.channels), (2_822_400, 2));
        assert!((info.duration_secs() - 3.0).abs() < 1e-9);
        let file = tagged(&id3.unwrap()).unwrap();
        let t = file.primary_tag().or_else(|| file.first_tag()).unwrap();
        assert_eq!(t.title().as_deref(), Some("Spem in alium"));
        assert_eq!(t.artist().as_deref(), Some("Tallis Scholars"));
        let meta = crate::tags::read_meta(&path).unwrap();
        assert_eq!(meta.title.as_deref(), Some("Spem in alium"));
        assert_eq!(
            (meta.sample_rate, meta.bit_depth),
            (Some(2_822_400), Some(1))
        );
        assert!((meta.duration_secs - 3.0).abs() < 1e-9);
        let _ = std::fs::remove_file(path);
    }
}
