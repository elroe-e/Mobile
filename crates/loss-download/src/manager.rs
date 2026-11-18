//! Download queue with parallel workers, pause/resume (HTTP Range),
//! automatic retries with backoff, and a queue that survives app restarts.

use crate::postprocess::{self, PostOptions};
use crate::{DownloadRequest, Error};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::sync::Notify;

const RUN: u8 = 0;
const PAUSE: u8 = 1;
const CANCEL: u8 = 2;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum JobStatus {
    Queued,
    Downloading,
    /// Tagging, unpacking, fetching lyrics.
    Processing,
    Paused,
    Done,
    Failed(String),
    Cancelled,
}

impl JobStatus {
    pub fn is_active(&self) -> bool {
        matches!(self, JobStatus::Downloading | JobStatus::Processing)
    }
    pub fn is_finished(&self) -> bool {
        matches!(self, JobStatus::Done | JobStatus::Failed(_) | JobStatus::Cancelled)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Job {
    id: u64,
    req: DownloadRequest,
    status: JobStatus,
    downloaded: u64,
    total: Option<u64>,
    attempts: u32,
    outputs: Vec<PathBuf>,
    #[serde(skip)]
    speed_bps: f64,
    #[serde(skip, default = "new_control")]
    control: Arc<AtomicU8>,
}

fn new_control() -> Arc<AtomicU8> {
    Arc::new(AtomicU8::new(RUN))
}

/// Read-only view of a job for the UI.
#[derive(Debug, Clone, PartialEq)]
pub struct JobSnapshot {
    pub id: u64,
    pub title: String,
    pub subtitle: String,
    pub status: JobStatus,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub speed_bps: f64,
    pub attempts: u32,
    pub outputs: Vec<PathBuf>,
}

impl JobSnapshot {
    pub fn progress(&self) -> Option<f64> {
        match (self.status.clone(), self.total) {
            (JobStatus::Done, _) => Some(1.0),
            (_, Some(t)) if t > 0 => Some((self.downloaded as f64 / t as f64).min(1.0)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ManagerConfig {
    pub dir: PathBuf,
    pub max_parallel: usize,
    pub max_retries: u32,
    pub auto_tag: bool,
    pub fetch_lyrics: bool,
    pub extract_zips: bool,
}

struct Inner {
    jobs: Mutex<Vec<Job>>,
    config: Mutex<ManagerConfig>,
    notify: Notify,
    client: reqwest::Client,
    state_path: PathBuf,
    /// Files that finished since the app last asked (to add to the library).
    completed: Mutex<Vec<PathBuf>>,
    next_id: Mutex<u64>,
}

#[derive(Clone)]
pub struct DownloadManager {
    inner: Arc<Inner>,
    rt: Arc<tokio::runtime::Runtime>,
}

impl DownloadManager {
    /// `state_path` is where the queue is saved between app launches.
    pub fn new(config: ManagerConfig, state_path: PathBuf) -> DownloadManager {
        let mut jobs: Vec<Job> = std::fs::read(&state_path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        for j in &mut jobs {
            if j.status.is_active() {
                j.status = JobStatus::Queued; // resume interrupted downloads
            }
        }
        let next_id = jobs.iter().map(|j| j.id).max().unwrap_or(0) + 1;
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("loss-net")
            .enable_all()
            .build()
            .expect("tokio runtime");
        let inner = Arc::new(Inner {
            jobs: Mutex::new(jobs),
            config: Mutex::new(config),
            notify: Notify::new(),
            client: crate::http::client(),
            state_path,
            completed: Mutex::new(Vec::new()),
            next_id: Mutex::new(next_id),
        });
        let mgr = DownloadManager {
            inner,
            rt: Arc::new(rt),
        };
        let sched = mgr.inner.clone();
        mgr.rt.spawn(scheduler(sched));
        mgr
    }

    /// The runtime, so sources can run their searches on it.
    pub fn runtime(&self) -> &tokio::runtime::Runtime {
        &self.rt
    }

    pub fn client(&self) -> reqwest::Client {
        self.inner.client.clone()
    }

    pub fn set_config(&self, config: ManagerConfig) {
        *lock(&self.inner.config) = config;
        self.inner.notify.notify_one();
    }

    pub fn enqueue(&self, reqs: Vec<DownloadRequest>) -> Vec<u64> {
        let mut ids = Vec::new();
        {
            let mut jobs = lock(&self.inner.jobs);
            let mut next = lock(&self.inner.next_id);
            for req in reqs {
                // Skip exact duplicates that are still pending.
                if jobs
                    .iter()
                    .any(|j| j.req.url == req.url && !j.status.is_finished())
                {
                    continue;
                }
                let id = *next;
                *next += 1;
                jobs.push(Job {
                    id,
                    req,
                    status: JobStatus::Queued,
                    downloaded: 0,
                    total: None,
                    attempts: 0,
                    outputs: vec![],
                    speed_bps: 0.0,
                    control: new_control(),
                });
                ids.push(id);
            }
        }
        self.inner.save();
        self.inner.notify.notify_one();
        ids
    }

    fn with_job(&self, id: u64, f: impl FnOnce(&mut Job)) {
        {
            let mut jobs = lock(&self.inner.jobs);
            if let Some(j) = jobs.iter_mut().find(|j| j.id == id) {
                f(j);
            }
        }
        self.inner.save();
        self.inner.notify.notify_one();
    }

    pub fn pause(&self, id: u64) {
        self.with_job(id, |j| match j.status {
            JobStatus::Downloading => j.control.store(PAUSE, Ordering::SeqCst),
            JobStatus::Queued => j.status = JobStatus::Paused,
            _ => {}
        });
    }

    pub fn resume(&self, id: u64) {
        self.with_job(id, |j| {
            if matches!(j.status, JobStatus::Paused | JobStatus::Failed(_) | JobStatus::Cancelled) {
                if matches!(j.status, JobStatus::Failed(_)) {
                    j.attempts = 0;
                }
                j.control = new_control();
                j.status = JobStatus::Queued;
            }
        });
    }

    pub fn cancel(&self, id: u64) {
        let dir = lock(&self.inner.config).dir.clone();
        self.with_job(id, |j| match j.status {
            JobStatus::Downloading => j.control.store(CANCEL, Ordering::SeqCst),
            JobStatus::Queued | JobStatus::Paused | JobStatus::Failed(_) => {
                j.status = JobStatus::Cancelled;
                let _ = std::fs::remove_file(part_path(&dir, &j.req.rel_path));
            }
            _ => {}
        });
    }

    pub fn pause_all(&self) {
        let ids: Vec<u64> = self.jobs().iter().map(|j| j.id).collect();
        ids.into_iter().for_each(|id| self.pause(id));
    }

    pub fn resume_all(&self) {
        let ids: Vec<u64> = self
            .jobs()
            .iter()
            .filter(|j| j.status == JobStatus::Paused)
            .map(|j| j.id)
            .collect();
        ids.into_iter().for_each(|id| self.resume(id));
    }

    /// Remove finished jobs from the list (files stay on disk).
    pub fn clear_finished(&self) {
        lock(&self.inner.jobs).retain(|j| !j.status.is_finished());
        self.inner.save();
    }

    pub fn jobs(&self) -> Vec<JobSnapshot> {
        lock(&self.inner.jobs)
            .iter()
            .map(|j| JobSnapshot {
                id: j.id,
                title: j
                    .req
                    .meta
                    .title
                    .clone()
                    .unwrap_or_else(|| file_name(&j.req.rel_path)),
                subtitle: [j.req.meta.artist.clone(), j.req.meta.album.clone()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" · "),
                status: j.status.clone(),
                downloaded: j.downloaded,
                total: j.total,
                speed_bps: j.speed_bps,
                attempts: j.attempts,
                outputs: j.outputs.clone(),
            })
            .collect()
    }

    /// Files completed since the last call.
    pub fn take_completed(&self) -> Vec<PathBuf> {
        std::mem::take(&mut *lock(&self.inner.completed))
    }

    /// Block until no job is queued or running (used by tests and the CLI).
    pub fn wait_idle(&self, timeout: Duration) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            let busy = lock(&self.inner.jobs)
                .iter()
                .any(|j| j.status.is_active() || j.status == JobStatus::Queued);
            if !busy {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn file_name(rel: &str) -> String {
    Path::new(rel)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(rel)
        .to_string()
}

/// Resolves a relative path inside `dir`, refusing anything that escapes it.
fn safe_join(dir: &Path, rel: &str) -> PathBuf {
    let mut out = dir.to_path_buf();
    for part in rel.split(['/', '\\']) {
        if part.is_empty() || part == "." || part == ".." {
            continue;
        }
        out.push(loss_core::sanitize_file_name(part));
    }
    out
}

fn part_path(dir: &Path, rel: &str) -> PathBuf {
    let p = safe_join(dir, rel);
    let mut s = p.into_os_string();
    s.push(".part");
    PathBuf::from(s)
}

/// Picks a free name: "Song.flac", "Song (2).flac"...
fn unique_path(p: PathBuf) -> PathBuf {
    if !p.exists() {
        return p;
    }
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("file").to_string();
    let ext = p.extension().and_then(|s| s.to_str()).map(|e| format!(".{e}")).unwrap_or_default();
    for n in 2..1000 {
        let cand = p.with_file_name(format!("{stem} ({n}){ext}"));
        if !cand.exists() {
            return cand;
        }
    }
    p
}

fn ext_for_content_type(ct: &str) -> Option<&'static str> {
    let ct = ct.split(';').next()?.trim().to_ascii_lowercase();
    Some(match ct.as_str() {
        "audio/mpeg" | "audio/mp3" => "mp3",
        "audio/flac" | "audio/x-flac" => "flac",
        "audio/ogg" | "application/ogg" => "ogg",
        "audio/opus" => "opus",
        "audio/mp4" | "audio/x-m4a" | "audio/aac" => "m4a",
        "audio/wav" | "audio/x-wav" | "audio/wave" => "wav",
        "application/zip" | "application/x-zip-compressed" => "zip",
        _ => return None,
    })
}

impl Inner {
    fn save(&self) {
        let jobs = lock(&self.jobs).clone();
        if let Some(dir) = self.state_path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(bytes) = serde_json::to_vec(&jobs) {
            let tmp = self.state_path.with_extension("tmp");
            if std::fs::write(&tmp, bytes).is_ok() {
                let _ = std::fs::rename(tmp, &self.state_path);
            }
        }
    }

    fn update(&self, id: u64, f: impl FnOnce(&mut Job)) {
        if let Some(j) = lock(&self.jobs).iter_mut().find(|j| j.id == id) {
            f(j);
        }
    }
}

async fn scheduler(inner: Arc<Inner>) {
    loop {
        let to_start: Vec<(u64, DownloadRequest, Arc<AtomicU8>)> = {
            let max = lock(&inner.config).max_parallel.max(1);
            let mut jobs = lock(&inner.jobs);
            let running = jobs.iter().filter(|j| j.status.is_active()).count();
            let mut picked = Vec::new();
            for j in jobs.iter_mut() {
                if running + picked.len() >= max {
                    break;
                }
                if j.status == JobStatus::Queued {
                    j.status = JobStatus::Downloading;
                    j.control.store(RUN, Ordering::SeqCst);
                    picked.push((j.id, j.req.clone(), j.control.clone()));
                }
            }
            picked
        };
        for (id, req, control) in to_start {
            let inner = inner.clone();
            tokio::spawn(async move {
                run_job(inner.clone(), id, req, control).await;
                inner.save();
                inner.notify.notify_one();
            });
        }
        let _ = tokio::time::timeout(Duration::from_secs(2), inner.notify.notified()).await;
    }
}

enum Outcome {
    Finished(PathBuf),
    Paused,
    Cancelled,
}

async fn run_job(inner: Arc<Inner>, id: u64, req: DownloadRequest, control: Arc<AtomicU8>) {
    let cfg = lock(&inner.config).clone();
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        inner.update(id, |j| j.attempts = attempt);
        match download_once(&inner, id, &req, &cfg.dir, &control).await {
            Ok(Outcome::Finished(path)) => {
                inner.update(id, |j| j.status = JobStatus::Processing);
                let opts = PostOptions {
                    auto_tag: cfg.auto_tag,
                    fetch_lyrics: cfg.fetch_lyrics,
                    extract_zips: cfg.extract_zips,
                };
                match postprocess::run(&inner.client, path, &req, &opts).await {
                    Ok(files) => {
                        lock(&inner.completed).extend(files.iter().cloned());
                        inner.update(id, |j| {
                            j.outputs = files;
                            j.status = JobStatus::Done;
                            j.speed_bps = 0.0;
                        });
                    }
                    Err(e) => inner.update(id, |j| j.status = JobStatus::Failed(e)),
                }
                return;
            }
            Ok(Outcome::Paused) => {
                inner.update(id, |j| {
                    j.status = JobStatus::Paused;
                    j.speed_bps = 0.0;
                });
                return;
            }
            Ok(Outcome::Cancelled) => {
                let _ = tokio::fs::remove_file(part_path(&cfg.dir, &req.rel_path)).await;
                inner.update(id, |j| {
                    j.status = JobStatus::Cancelled;
                    j.speed_bps = 0.0;
                    j.downloaded = 0;
                });
                return;
            }
            Err((e, retryable)) => {
                log::warn!("download {id} attempt {attempt} failed: {e}");
                if !retryable || attempt > cfg.max_retries {
                    inner.update(id, |j| {
                        j.status = JobStatus::Failed(e.to_string());
                        j.speed_bps = 0.0;
                    });
                    return;
                }
                let wait = Duration::from_secs((1u64 << attempt.min(5)).min(30));
                let deadline = Instant::now() + wait;
                while Instant::now() < deadline {
                    if control.load(Ordering::SeqCst) != RUN {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
                match control.load(Ordering::SeqCst) {
                    PAUSE => {
                        inner.update(id, |j| j.status = JobStatus::Paused);
                        return;
                    }
                    CANCEL => {
                        inner.update(id, |j| j.status = JobStatus::Cancelled);
                        return;
                    }
                    _ => {}
                }
            }
        }
    }
}

type AttemptError = (Error, bool);

async fn download_once(
    inner: &Inner,
    id: u64,
    req: &DownloadRequest,
    dir: &Path,
    control: &AtomicU8,
) -> Result<Outcome, AttemptError> {
    let part = part_path(dir, &req.rel_path);
    if let Some(parent) = part.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| (Error::from(e), false))?;
    }
    let have = tokio::fs::metadata(&part).await.map(|m| m.len()).unwrap_or(0);

    let mut request = inner.client.get(&req.url);
    for (k, v) in &req.headers {
        request = request.header(k.as_str(), v.as_str());
    }
    if have > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={have}-"));
    }
    let resp = request.send().await.map_err(|e| (Error::from(e), true))?;
    let status = resp.status();

    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE && have > 0 {
        // Already fully downloaded before the interruption.
        return finish(&part, &req.rel_path, dir, &content_type).await;
    }
    if !status.is_success() {
        let retryable = status.is_server_error()
            || status == reqwest::StatusCode::TOO_MANY_REQUESTS
            || status == reqwest::StatusCode::REQUEST_TIMEOUT;
        let msg = match status.as_u16() {
            401 | 403 => format!("access denied ({status}): check the link or your login"),
            404 => "file not found (404)".to_string(),
            _ => format!("server answered {status}"),
        };
        return Err((Error::Other(msg), retryable));
    }
    if content_type.starts_with("text/html") {
        return Err((
            Error::Other("the link opens a web page, not a file. Use the page's real download link.".into()),
            false,
        ));
    }

    let resumed = status == reqwest::StatusCode::PARTIAL_CONTENT && have > 0;
    let start = if resumed { have } else { 0 };
    let total = resp.content_length().map(|l| l + start);
    inner.update(id, |j| {
        j.downloaded = start;
        j.total = total;
    });

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(resumed)
        .truncate(!resumed)
        .open(&part)
        .await
        .map_err(|e| (Error::from(e), false))?;

    let mut stream = resp.bytes_stream();
    let mut downloaded = start;
    let mut window_start = Instant::now();
    let mut window_bytes = 0u64;
    while let Some(chunk) = stream.next().await {
        match control.load(Ordering::SeqCst) {
            PAUSE => {
                file.flush().await.ok();
                return Ok(Outcome::Paused);
            }
            CANCEL => {
                drop(file);
                return Ok(Outcome::Cancelled);
            }
            _ => {}
        }
        let chunk = chunk.map_err(|e| {
            (Error::from(e), true)
        })?;
        file.write_all(&chunk)
            .await
            .map_err(|e| (Error::from(e), false))?;
        downloaded += chunk.len() as u64;
        window_bytes += chunk.len() as u64;
        let elapsed = window_start.elapsed();
        if elapsed >= Duration::from_millis(250) {
            let speed = window_bytes as f64 / elapsed.as_secs_f64();
            inner.update(id, |j| {
                j.downloaded = downloaded;
                j.speed_bps = speed;
            });
            window_start = Instant::now();
            window_bytes = 0;
        }
    }
    file.flush().await.map_err(|e| (Error::from(e), true))?;
    drop(file);
    inner.update(id, |j| j.downloaded = downloaded);
    if let Some(t) = total {
        if downloaded < t {
            return Err((Error::Other("connection closed early".into()), true));
        }
    }
    finish(&part, &req.rel_path, dir, &content_type).await
}

async fn finish(part: &Path, rel: &str, dir: &Path, content_type: &str) -> Result<Outcome, AttemptError> {
    let mut target = safe_join(dir, rel);
    if target.extension().is_none() {
        if let Some(ext) = ext_for_content_type(content_type) {
            target.set_extension(ext);
        }
    }
    let target = unique_path(target);
    tokio::fs::rename(part, &target)
        .await
        .map_err(|e| (Error::from(e), false))?;
    Ok(Outcome::Finished(target))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_join_blocks_escape() {
        let d = Path::new("/music");
        assert_eq!(safe_join(d, "../../etc/passwd"), PathBuf::from("/music/etc/passwd"));
        assert_eq!(safe_join(d, "A/B/c.mp3"), PathBuf::from("/music/A/B/c.mp3"));
    }
}
