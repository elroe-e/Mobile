//! End-to-end tests of the download manager against a local HTTP server.

use loss_download::sources::SourceKind;
use loss_download::{DownloadManager, DownloadRequest, JobStatus, ManagerConfig};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Serves `body` with Range support, slowly, failing the first `fail_first` requests with 503.
fn serve(body: Vec<u8>, fail_first: usize) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let n = h.fetch_add(1, Ordering::SeqCst);
            let body = body.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(s.try_clone().unwrap());
                let mut range_start = 0usize;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    let l = line.to_ascii_lowercase();
                    if let Some(r) = l.strip_prefix("range: bytes=") {
                        range_start = r.trim().trim_end_matches('-').parse().unwrap_or(0);
                    }
                }
                if n < fail_first {
                    let _ = s.write_all(b"HTTP/1.1 503 Busy\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    return;
                }
                let part = &body[range_start.min(body.len())..];
                let head = if range_start > 0 {
                    format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Type: audio/wav\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                        part.len(), range_start, body.len() - 1, body.len()
                    )
                } else {
                    format!("HTTP/1.1 200 OK\r\nContent-Type: audio/wav\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", part.len())
                };
                if s.write_all(head.as_bytes()).is_err() {
                    return;
                }
                for chunk in part.chunks(8192) {
                    if s.write_all(chunk).is_err() {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
                let mut _sink = Vec::new();
                let _ = s.read_to_end(&mut _sink);
            });
        }
    });
    (format!("http://{addr}/tone.wav"), hits)
}

fn wav(secs: f32) -> Vec<u8> {
    let rate = 8000u32;
    let frames = (rate as f32 * secs) as u32;
    let data_len = frames * 2;
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..frames {
        b.extend_from_slice(&(((i % 40) as i16 - 20) * 500).to_le_bytes());
    }
    b
}

fn manager(dir: &std::path::Path, retries: u32) -> DownloadManager {
    DownloadManager::new(
        ManagerConfig {
            dir: dir.join("music"),
            max_parallel: 2,
            max_retries: retries,
            auto_tag: true,
            fetch_lyrics: false,
            extract_zips: true,
        },
        dir.join("queue.json"),
    )
}

fn request(url: &str) -> DownloadRequest {
    let meta = loss_core::tags::TrackMeta {
        title: Some("Test Tone".into()),
        artist: Some("Loss Lab".into()),
        album: Some("Signals".into()),
        track_no: Some(1),
        ..Default::default()
    };
    DownloadRequest {
        url: url.into(),
        rel_path: DownloadRequest::build_rel_path(&meta, "tone", "wav"),
        meta,
        cover_url: None,
        source: SourceKind::Direct,
        headers: vec![],
    }
}

#[test]
fn downloads_pauses_resumes_and_tags() {
    let dir = std::env::temp_dir().join(format!("loss-dl-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let body = wav(30.0); // ~480 KB, served slowly
    let (url, hits) = serve(body.clone(), 0);
    let m = manager(&dir, 3);
    let id = m.enqueue(vec![request(&url)])[0];

    // Let it get partway, then pause.
    for _ in 0..200 {
        std::thread::sleep(Duration::from_millis(20));
        if m.jobs()[0].downloaded > 20_000 {
            break;
        }
    }
    m.pause(id);
    for _ in 0..100 {
        if m.jobs()[0].status == JobStatus::Paused {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let paused = m.jobs()[0].clone();
    assert_eq!(paused.status, JobStatus::Paused);
    assert!(paused.downloaded > 0 && paused.downloaded < body.len() as u64);

    m.resume(id);
    assert!(m.wait_idle(Duration::from_secs(30)));
    let done = m.jobs()[0].clone();
    assert_eq!(done.status, JobStatus::Done, "{:?}", done.status);
    assert!(hits.load(Ordering::SeqCst) >= 2, "resume should use a second (Range) request");

    let out = &done.outputs[0];
    assert!(out.ends_with("music/Loss Lab/Signals/01 Test Tone.wav"), "{}", out.display());
    let meta = loss_core::tags::read_meta(out).unwrap();
    assert_eq!(meta.title.as_deref(), Some("Test Tone"));
    assert_eq!(meta.artist.as_deref(), Some("Loss Lab"));
    assert!((meta.duration_secs - 30.0).abs() < 0.1);
    // Audio bytes identical after resume (tags are appended in a separate chunk).
    let got = std::fs::read(out).unwrap();
    assert!(got.windows(1000).any(|w| w == &body[200_000..201_000]));
    assert_eq!(m.take_completed().len(), 1);

    // Queue file survives a restart and duplicate detection ignores finished jobs.
    drop(m);
    let m2 = manager(&dir, 3);
    assert_eq!(m2.jobs().len(), 1);
    m2.clear_finished();
    assert!(m2.jobs().is_empty());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn retries_server_errors_then_succeeds() {
    let dir = std::env::temp_dir().join(format!("loss-dl-retry-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (url, hits) = serve(wav(1.0), 2);
    let m = manager(&dir, 3);
    m.enqueue(vec![request(&url)]);
    assert!(m.wait_idle(Duration::from_secs(30)));
    assert_eq!(m.jobs()[0].status, JobStatus::Done);
    assert_eq!(hits.load(Ordering::SeqCst), 3);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn gives_up_on_404() {
    let dir = std::env::temp_dir().join(format!("loss-dl-404-{}", std::process::id()));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for s in listener.incoming().flatten() {
            let mut s = s;
            let mut buf = [0u8; 1024];
            let _ = s.read(&mut buf);
            let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        }
    });
    let m = manager(&dir, 3);
    m.enqueue(vec![request(&format!("http://{addr}/missing.mp3"))]);
    assert!(m.wait_idle(Duration::from_secs(10)));
    let j = m.jobs()[0].clone();
    assert!(matches!(j.status, JobStatus::Failed(ref e) if e.contains("404")), "{:?}", j.status);
    assert_eq!(j.attempts, 1, "404 must not be retried");
    std::fs::remove_dir_all(&dir).ok();
}

/// Live check against archive.org; run with `cargo test -- --ignored`.
#[test]
#[ignore]
fn live_internet_archive() {
    let m = manager(&std::env::temp_dir().join("loss-live"), 1);
    let client = m.client();
    let items = m
        .runtime()
        .block_on(loss_download::sources::archive::search(&client, "netlabel electronic", true, 1))
        .unwrap();
    assert!(!items.is_empty());
    println!("first result: {} — {:?}", items[0].title, items[0].license);
    let loss_download::sources::ItemAction::Open(c) = &items[0].action else { panic!() };
    let files = m
        .runtime()
        .block_on(loss_download::sources::archive::open(&client, &c.id))
        .unwrap();
    println!("{} files, first: {:?}", files.len(), files.first().map(|f| (&f.title, &f.format)));
}
