// Used by the Android media thread; desktop notifications come with M9-08.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

//! What the downloads notification shows, derived from the download queue.

use loss_download::{JobSnapshot, JobStatus};

#[derive(Clone, Debug, PartialEq)]
pub enum DownloadsState {
    /// Nothing to show.
    Idle,
    /// Downloads are running: keep the background service alive.
    Running,
    /// Some downloads are waiting, all paused.
    Paused,
    /// Everything finished since downloads last ran.
    Finished,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DownloadsNotice {
    pub state: DownloadsState,
    pub title: String,
    pub text: String,
    /// 0..=1000, or -1 when the total size isn't known yet.
    pub progress: i32,
}

fn speed(bps: f64) -> String {
    if bps >= 1e6 {
        format!("{:.1} MB/s", bps / 1e6)
    } else {
        format!("{:.0} KB/s", bps / 1e3)
    }
}

/// `ran` is whether downloads were running earlier, so a summary is shown
/// only after a run (not for old finished jobs at app start).
pub fn downloads_notice(jobs: &[JobSnapshot], ran: bool) -> DownloadsNotice {
    let active: Vec<&JobSnapshot> = jobs
        .iter()
        .filter(|j| j.status.is_active() || j.status == JobStatus::Queued)
        .collect();
    let paused = jobs
        .iter()
        .filter(|j| j.status == JobStatus::Paused)
        .count();
    let done = jobs.iter().filter(|j| j.status == JobStatus::Done).count();
    let failed = jobs
        .iter()
        .filter(|j| matches!(j.status, JobStatus::Failed(_)))
        .count();

    if !active.is_empty() {
        let current = active
            .iter()
            .find(|j| j.status == JobStatus::Downloading)
            .or(active.first())
            .map(|j| j.title.clone())
            .unwrap_or_default();
        let (got, total): (u64, u64) = active
            .iter()
            .filter_map(|j| j.total.map(|t| (j.downloaded, t)))
            .fold((0, 0), |(a, b), (d, t)| (a + d, b + t));
        let progress = if total > 0 {
            ((got as f64 / total as f64) * 1000.0).round() as i32
        } else {
            -1
        };
        let bps: f64 = active.iter().map(|j| j.speed_bps).sum();
        let left = active.len();
        let mut text = if left == 1 {
            "1 song left".to_string()
        } else {
            format!("{left} songs left")
        };
        if bps > 0.0 {
            text.push_str(&format!(" · {}", speed(bps)));
        }
        return DownloadsNotice {
            state: DownloadsState::Running,
            title: format!("Downloading {current}"),
            text,
            progress: progress.clamp(-1, 1000),
        };
    }
    if paused > 0 {
        return DownloadsNotice {
            state: DownloadsState::Paused,
            title: "Downloads paused".into(),
            text: if paused == 1 {
                "1 song waiting".into()
            } else {
                format!("{paused} songs waiting")
            },
            progress: -1,
        };
    }
    if ran && done + failed > 0 {
        let text = match (done, failed) {
            (d, 0) => format!("{d} done"),
            (0, f) => format!("{f} failed"),
            (d, f) => format!("{d} done, {f} failed"),
        };
        return DownloadsNotice {
            state: DownloadsState::Finished,
            title: "Downloads finished".into(),
            text,
            progress: -1,
        };
    }
    DownloadsNotice {
        state: DownloadsState::Idle,
        title: String::new(),
        text: String::new(),
        progress: -1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(
        id: u64,
        title: &str,
        status: JobStatus,
        got: u64,
        total: Option<u64>,
        bps: f64,
    ) -> JobSnapshot {
        JobSnapshot {
            id,
            title: title.into(),
            subtitle: String::new(),
            status,
            downloaded: got,
            total,
            speed_bps: bps,
            attempts: 1,
            outputs: vec![],
        }
    }

    #[test]
    fn running_paused_finished_idle() {
        let running = vec![
            job(1, "Song A", JobStatus::Downloading, 500, Some(1000), 1.5e6),
            job(2, "Song B", JobStatus::Queued, 0, None, 0.0),
            job(3, "Song C", JobStatus::Done, 10, Some(10), 0.0),
        ];
        let n = downloads_notice(&running, false);
        assert_eq!(n.state, DownloadsState::Running);
        assert_eq!(n.title, "Downloading Song A");
        assert_eq!(n.text, "2 songs left · 1.5 MB/s");
        assert_eq!(n.progress, 500);

        let paused = vec![job(1, "A", JobStatus::Paused, 5, Some(10), 0.0)];
        assert_eq!(
            downloads_notice(&paused, true).state,
            DownloadsState::Paused
        );

        let finished = vec![
            job(1, "A", JobStatus::Done, 1, Some(1), 0.0),
            job(2, "B", JobStatus::Failed("404".into()), 0, None, 0.0),
        ];
        let n = downloads_notice(&finished, true);
        assert_eq!(n.state, DownloadsState::Finished);
        assert_eq!(n.text, "1 done, 1 failed");
        // Old results at app start don't produce a notification.
        assert_eq!(
            downloads_notice(&finished, false).state,
            DownloadsState::Idle
        );
    }
}
