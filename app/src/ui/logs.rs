//! Settings → Logs and diagnostics: the app log, the last crash report, and
//! device/audio-output details for bug reports and DAC checks.

use super::icons::Icon;
use super::settings::{SettingsNav, SettingsPage};
use super::{copy_text, share_text, state};
use crate::diag;
use dioxus::prelude::*;
use std::time::Duration;

#[derive(Clone, Copy, PartialEq)]
enum Filter {
    All,
    Problems,
    Info,
    Debug,
}

impl Filter {
    fn label(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Problems => "Problems",
            Filter::Info => "Info",
            Filter::Debug => "Debug",
        }
    }
    fn keeps(self, level: log::Level) -> bool {
        match self {
            Filter::All => true,
            Filter::Problems => level <= log::Level::Warn,
            Filter::Info => level == log::Level::Info,
            Filter::Debug => level >= log::Level::Debug,
        }
    }
}

const SHOWN: usize = 400;

#[component]
pub fn LogsPage() -> Element {
    let st = state();
    let mut page = use_context::<SettingsNav>().0;
    let mut query = use_signal(String::new);
    let mut filter = use_signal(|| Filter::All);
    let mut tick = use_signal(|| 0u64);
    let mut crash = use_signal(|| diag::pending_crash().map(|(_, text)| text));
    let mut details = use_signal(|| None::<String>);

    // New lines appear while the page is open.
    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            tick += 1;
        }
    });
    let _ = tick();

    let q = query().to_lowercase();
    let f = filter();
    let all = diag::lines();
    let total = all.len();
    let lines: Vec<_> = all
        .into_iter()
        .rev()
        .filter(|l| f.keeps(l.level))
        .filter(|l| {
            q.is_empty()
                || l.message.to_lowercase().contains(&q)
                || l.target.to_lowercase().contains(&q)
        })
        .take(SHOWN)
        .collect();

    rsx! {
        button { class: "back", onclick: move |_| page.set(SettingsPage::Main), Icon { name: "back" } }
        header { class: "page-header", h1 { "Logs" } }

        if let Some(report) = crash() {
            h2 { class: "section-title", "Last crash" }
            div { class: "card",
                p { class: "small muted", "Loss closed unexpectedly. Sending this report helps fix it." }
                pre { class: "pre-box", "{report}" }
                div { class: "hero-actions start",
                    button {
                        class: "pill",
                        onclick: {
                            let r = report.clone();
                            move |_| copy_text(st, "Loss crash report", &r)
                        },
                        Icon { name: "check", size: 18 }
                        "Copy"
                    }
                    button {
                        class: "pill",
                        onclick: {
                            let r = report.clone();
                            move |_| share_text(st, "Loss crash report", &r)
                        },
                        "Share"
                    }
                    button {
                        class: "pill",
                        onclick: move |_| {
                            diag::dismiss_crashes();
                            crash.set(None);
                        },
                        "Dismiss"
                    }
                }
            }
        }

        h2 { class: "section-title", "This device" }
        div { class: "card",
            p { class: "small muted",
                "Phone model, Android version and every audio output with the sample rates and formats it accepts, including USB DACs."
            }
            if let Some(text) = details() {
                pre { class: "pre-box", "{text}" }
            }
            div { class: "hero-actions start",
                button {
                    class: "pill",
                    onclick: move |_| details.set(Some(crate::platform::diagnostics())),
                    if details().is_some() { "Refresh" } else { "Show" }
                }
                button {
                    class: "pill",
                    onclick: move |_| copy_text(st, "Loss diagnostics", &full_report()),
                    "Copy"
                }
                button {
                    class: "pill",
                    onclick: move |_| share_text(st, "Loss diagnostics", &full_report()),
                    "Share"
                }
            }
        }

        h2 { class: "section-title", "App log" }
        div { class: "search-box",
            Icon { name: "search", size: 22 }
            input {
                r#type: "search",
                placeholder: "Search the log",
                initial_value: "{query}",
                oninput: move |e| query.set(e.value()),
            }
        }
        div { class: "chips",
            for opt in [Filter::All, Filter::Problems, Filter::Info, Filter::Debug] {
                button {
                    class: if f == opt { "chip selected" } else { "chip" },
                    onclick: move |_| filter.set(opt),
                    "{opt.label()}"
                }
            }
        }
        div { class: "toolbar",
            span { class: "muted small", "{lines.len()} of {total} lines, newest first" }
            div { class: "header-actions",
                button { class: "text-btn", onclick: move |_| copy_text(st, "Loss log", &diag::export_text()), "Copy" }
                button { class: "text-btn", onclick: move |_| share_text(st, "Loss log", &diag::export_text()), "Share" }
                button {
                    class: "text-btn",
                    onclick: move |_| {
                        diag::clear();
                        st.toast("Log cleared");
                    },
                    "Clear"
                }
            }
        }
        if lines.is_empty() {
            p { class: "muted pad", "Nothing logged yet." }
        }
        div { class: "log-list",
            for (i , l) in lines.iter().enumerate() {
                div { key: "{i}-{l.time_ms}", class: "log-line",
                    span { class: "log-meta",
                        span { class: "log-level lvl-{l.level.as_str().to_lowercase()}", "{l.level}" }
                        " {l.target}"
                    }
                    span { class: "log-msg", "{l.message}" }
                }
            }
        }
    }
}

/// Device details plus the log, for bug reports.
fn full_report() -> String {
    format!(
        "{}\nSettings: detailed logging {}\n\n{}",
        crate::platform::diagnostics(),
        if crate::services::get().settings().detailed_logging {
            "on"
        } else {
            "off"
        },
        diag::export_text()
    )
}
