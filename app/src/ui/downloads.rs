//! Downloads: progress, speed, pause/resume, cancel and retry.

use super::icons::Icon;
use super::{format_bytes, state, Tab};
use crate::services;
use dioxus::prelude::*;
use loss_download::{JobSnapshot, JobStatus};

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
            h1 { "Downloads" }
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
            p { class: "muted small pad", "{format_bytes(total_speed as u64)}/s · saving to {dir}" }
        } else {
            p { class: "muted small pad", "Saving to {dir}" }
        }
        if jobs.is_empty() {
            div { class: "empty",
                Icon { name: "download", size: 64 }
                h2 { "No downloads" }
                p { "Find free music, podcasts, your own server's albums, or paste a purchase link." }
                button { class: "primary", onclick: move |_| st.tab.set(Tab::Discover), "Go to Discover" }
            }
        }
        if !active.is_empty() {
            h2 { class: "section-title", "In progress ({active.len()})" }
            div { class: "list",
                for j in active {
                    JobRow { key: "{j.id}", job: j.clone() }
                }
            }
        }
        if !finished.is_empty() {
            div { class: "section-row",
                h2 { class: "section-title", "Finished ({finished.len()})" }
                button { class: "pill", onclick: move |_| services::get().downloads.clear_finished(), "Clear" }
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
        JobStatus::Queued => "Waiting".to_string(),
        JobStatus::Downloading if job.attempts > 1 => format!("Downloading (try {})", job.attempts),
        JobStatus::Downloading => format!("{}/s", format_bytes(job.speed_bps as u64)),
        JobStatus::Processing => "Adding tags, cover and lyrics…".into(),
        JobStatus::Paused => "Paused".into(),
        JobStatus::Done => "Done · in your library".into(),
        JobStatus::Failed(e) => format!("Failed: {e}"),
        JobStatus::Cancelled => "Cancelled".into(),
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
