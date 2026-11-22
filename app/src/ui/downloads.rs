//! Downloads: progress, speed, pause/resume, cancel and retry.

use super::icons::Icon;
use super::{format_bytes, state, Tab};
use crate::services;
use dioxus::prelude::*;
use loss_download::{JobSnapshot, JobStatus};
use loss_i18n::t;

#[component]
pub fn DownloadsTab() -> Element {
    let mut st = state();
    let jobs = st.jobs.read().clone();
    let active: Vec<&JobSnapshot> = jobs.iter().filter(|j| !j.status.is_finished()).collect();
    let finished: Vec<&JobSnapshot> = jobs.iter().filter(|j| j.status.is_finished()).collect();
    let any_paused = jobs.iter().any(|j| j.status == JobStatus::Paused);
    let any_running = jobs
        .iter()
        .any(|j| j.status.is_active() || j.status == JobStatus::Queued);
    let total_speed: f64 = jobs.iter().map(|j| j.speed_bps).sum();
    let dir = services::get()
        .settings()
        .download_dir
        .display()
        .to_string();

    rsx! {
        header { class: "page-header",
            h1 { {t!("tab-downloads")} }
            div { class: "header-actions",
                if any_running {
                    button { class: "pill", onclick: move |_| services::get().downloads.pause_all(), Icon { name: "pause", size: 18 } " Pause all" }
                }
                if any_paused {
                    button { class: "pill", onclick: move |_| services::get().downloads.resume_all(), Icon { name: "play", size: 18 } " Resume all" }
                }
            }
        }
        if total_speed > 0.0 {
            p { class: "muted small pad", {t!("downloads-speed-saving", speed = format_bytes(total_speed as u64), folder = dir.clone())} }
        } else {
            p { class: "muted small pad", {t!("downloads-saving", folder = dir.clone())} }
        }
        if jobs.is_empty() {
            div { class: "empty",
                Icon { name: "download", size: 64 }
                h2 { {t!("downloads-empty-title")} }
                p { {t!("downloads-empty-text")} }
                button { class: "primary", onclick: move |_| st.tab.set(Tab::Discover), {t!("downloads-go-discover")} }
            }
        }
        if !active.is_empty() {
            h2 { class: "section-title", {t!("downloads-in-progress", count = active.len())} }
            div { class: "list",
                for j in active {
                    JobRow { key: "{j.id}", job: j.clone() }
                }
            }
        }
        if !finished.is_empty() {
            div { class: "section-row",
                h2 { class: "section-title", {t!("downloads-finished", count = finished.len())} }
                button { class: "pill", onclick: move |_| services::get().downloads.clear_finished(), {t!("action-clear")} }
            }
            div { class: "list",
                for j in finished {
                    JobRow { key: "{j.id}", job: j.clone() }
                }
            }
        }
    }
}

#[component]
fn JobRow(job: JobSnapshot) -> Element {
    let id = job.id;
    let pct = job.progress().map(|p| p * 100.0);
    let size = match job.total {
        Some(t) => format!("{} / {}", format_bytes(job.downloaded), format_bytes(t)),
        None if job.downloaded > 0 => format_bytes(job.downloaded),
        None => String::new(),
    };
    let status_text = match &job.status {
        JobStatus::Queued => t!("job-waiting"),
        JobStatus::Downloading if job.attempts > 1 => t!("job-retrying", attempt = job.attempts),
        JobStatus::Downloading => format!("{}/s", format_bytes(job.speed_bps as u64)),
        JobStatus::Processing => t!("job-processing"),
        JobStatus::Paused => t!("job-paused"),
        JobStatus::Done => t!("job-done"),
        JobStatus::Failed(e) => t!("job-failed", error = e.to_string()),
        JobStatus::Cancelled => t!("job-cancelled"),
    };
    let failed = matches!(job.status, JobStatus::Failed(_));
    rsx! {
        div { class: "row job",
            div { class: "row-text",
                div { class: "title", "{job.title}" }
                if !job.subtitle.is_empty() {
                    div { class: "subtitle", "{job.subtitle}" }
                }
                if !job.status.is_finished() {
                    div { class: "progress",
                        div {
                            class: if pct.is_none() { "bar indeterminate" } else { "bar" },
                            style: "width: {pct.unwrap_or(30.0)}%;",
                        }
                    }
                }
                div { class: if failed { "small error" } else { "small muted" },
                    "{status_text}"
                    if !size.is_empty() && !job.status.is_finished() { " · {size}" }
                }
            }
            div { class: "job-actions",
                match job.status {
                    JobStatus::Downloading | JobStatus::Queued => rsx! {
                        button { class: "icon-btn small", onclick: move |_| services::get().downloads.pause(id), Icon { name: "pause", size: 22 } }
                        button { class: "icon-btn small", onclick: move |_| services::get().downloads.cancel(id), Icon { name: "close", size: 22 } }
                    },
                    JobStatus::Paused => rsx! {
                        button { class: "icon-btn small", onclick: move |_| services::get().downloads.resume(id), Icon { name: "play", size: 22 } }
                        button { class: "icon-btn small", onclick: move |_| services::get().downloads.cancel(id), Icon { name: "close", size: 22 } }
                    },
                    JobStatus::Failed(_) | JobStatus::Cancelled => rsx! {
                        button { class: "icon-btn small", onclick: move |_| services::get().downloads.resume(id), Icon { name: "retry", size: 22 } }
                    },
                    JobStatus::Done => rsx! {
                        span { class: "done-check", Icon { name: "check", size: 22 } }
                    },
                    JobStatus::Processing => rsx! { span { class: "spinner" } },
                }
            }
        }
    }
}
